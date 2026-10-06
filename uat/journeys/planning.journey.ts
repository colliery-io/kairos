// J2 — "A strategy is broken down until it is work on a board"
// (KAIROS-I-0011 D4). alice plans in the GUI (strategy, initiative),
// decomposes from the CLI (tasks, a blocks edge; the edges themselves go
// over the API — the CLI has no relationships verb), reads the rollups in
// the GUI (progress bar, graph), and bob watches the board move live.
// The editorial lifecycle is a document feature (A-0018), so the story
// ends with a design note attached to the initiative moving to review.
import { expect, type Page } from '@playwright/test';
import { named } from '../run/context';
import { journey, step } from '../run/narrate';
import { card, cardIn, openBoard, openItem, panel } from '../surfaces/gui';

async function createFromHeader(page: Page, kind: string, title: string): Promise<string> {
  await page.getByRole('button', { name: `New ${kind}`, exact: true }).click();
  const modal = page.locator('.cl-modal');
  await expect(modal.locator('.cl-modal__title')).toHaveText(`New ${kind}`);
  await modal.locator('input.cl-input').first().fill(title);
  await modal.getByRole('button', { name: 'Create' }).click();
  const created = card(page, title);
  await expect(created).toBeVisible();
  const codeLink = created.locator('a.kairos-card__code');
  await expect(codeLink).toHaveText(/[A-Z0-9]+-[A-Z]-\d{4}/);
  return (await codeLink.innerText()).trim();
}

journey(
  'planning',
  'A strategy is broken down until it is work on a board',
  { humans: ['alice', 'bob'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    const bob = cast.human('bob');
    const strategyTitle = named('strategy: self-serve billing');
    const initiativeTitle = named('initiative: invoice exports');
    let strategy = '';
    let initiative = '';
    let bound = '';   // the task bound to payments-api (blocked)
    let blocker = ''; // the task that blocks it

    await step(alice, 'creates a strategy on the Strategy board from the GUI', async () => {
      const page = await alice.gui();
      await openBoard(page, 'strategy');
      strategy = await createFromHeader(page, 'strategy', strategyTitle);
      const api = await alice.api();
      ledger.add({ kind: 'strategy', label: strategy, delete: async () => { await api.delete(`/api/strategies/${strategy}`); } });
      return { short_code: strategy, column: await columnOfCard(page, strategy) };
    });

    await step(alice, 'creates an initiative under it on the Initiatives board', async () => {
      const page = await alice.gui();
      await openBoard(page, 'initiatives');
      initiative = await createFromHeader(page, 'initiative', initiativeTitle);
      const api = await alice.api();
      ledger.add({ kind: 'initiative', label: initiative, delete: async () => { await api.delete(`/api/initiatives/${initiative}`); } });
      // The create modal has no parent picker; the Graph tab's "Manage
      // links" panel is where a person attaches it to the strategy.
      await card(page, initiativeTitle).locator('a.kairos-card__code').click();
      await page.waitForURL(new RegExp(`/items/${initiative}`));
      await expect(page.getByText(initiative, { exact: true }).first()).toBeVisible();
      // The tab anchors are built from the resolved code, never an empty one.
      // COLLIERY-T-1836: the Aurora route tabs are links with role="tab".
      const graphTab = page.getByRole('tab', { name: 'Graph', exact: true });
      await expect(graphTab).toHaveAttribute('href', `/items/${initiative}?view=graph`);
      await graphTab.click();
      await page.waitForURL(/view=graph/);
      const links = panel(page, 'Manage links');
      await expect(links).toBeVisible();
      await links.getByRole('button', { name: 'target', exact: true }).click();
      await links.locator('input').first().fill(strategy);
      await links.locator('select').selectOption('parent');
      await links.getByRole('button', { name: 'Create link' }).click();
      await expect(links.getByText(`parent ← ${strategy}`)).toBeVisible({ timeout: 10_000 });
      return { short_code: initiative, parent: strategy, linked_via: 'Manage links panel' };
    });

    // KAIROS-T-0321: the team is known before the work is divided.
    await step(alice, 'names the platform team on the initiative over MCP before it has tasks', async () => {
      const mcp = await alice.mcp();
      const before = await mcp.call('get_item', { short_code: initiative });
      expect(before).toContain('- teams: none');
      const set = await mcp.call('set_team', { short_code: initiative, team: 'platform' });
      expect(set).toBe(`Set the team platform on ${initiative} by hand.`);
      const after = await mcp.call('get_item', { short_code: initiative });
      expect(after).toContain('- teams: platform (set by hand)');
      return { short_code: initiative, teams: 'platform (set by hand)' };
    });

    await step(alice, 'decomposes it from the CLI into two platform tasks, one bound to payments-api, the other blocking it', async () => {
      const cli = await alice.cli();
      const api = await alice.api();
      const platform = await api.boardBySlug('platform-delivery');
      const t1 = await cli.json(['tasks', 'create', '--board', platform.id, '--repo', 'payments-api', '--title', named('task: export endpoint')]);
      const t2 = await cli.json(['tasks', 'create', '--board', platform.id, '--title', named('task: export schema agreed')]);
      bound = t1.short_code;
      blocker = t2.short_code;
      for (const code of [bound, blocker]) {
        ledger.add({ kind: 'task', label: code, delete: async () => { await api.delete(`/api/tasks/${code}`); } });
        await api.post('/api/relationships', { source_short_code: initiative, target_short_code: code, relationship: 'parent' });
      }
      await api.post('/api/relationships', { source_short_code: blocker, target_short_code: bound, relationship: 'blocks' });
      expect(t1.board_id).toBe(platform.id);
      expect(t1.repository?.slug).toBe('payments-api');
      return { bound_task: bound, blocker_task: blocker, board: 'platform-delivery', edges: 'parent ×2 (API), blocks (API)' };
    });

    await step(alice, 'sees the platform team come from the tasks too, then clears the hand-set one over MCP', async () => {
      const mcp = await alice.mcp();
      const both = await mcp.call('get_item', { short_code: initiative });
      expect(both).toContain('- teams: platform (from tasks, set by hand)');
      const listed = await mcp.call('board_items', { board: 'initiatives', team: 'platform' });
      expect(listed).toContain(`- ${initiative} [initiative]`);
      expect(listed).toContain('[teams: platform]');
      const cleared = await mcp.call('clear_team', { short_code: initiative, team: 'platform' });
      expect(cleared).toBe(`Cleared the team platform set by hand on ${initiative}.`);
      const after = await mcp.call('get_item', { short_code: initiative });
      expect(after).toContain('- teams: platform (from tasks)');
      return { short_code: initiative, teams: 'platform (from tasks)' };
    });

    await step(alice, 'opens the initiative and reads the 0-of-2 progress bar', async () => {
      const page = await alice.gui();
      await openItem(page, initiative);
      const bar = page.locator('.kairos-progress');
      await expect(bar).toBeVisible();
      await expect(bar).toContainText('0 of 2 done');
      return { progress: (await bar.innerText()).replace(/\s+/g, ' ').trim() };
    });

    await step(alice, 'opens the Graph tab and sees the initiative, both tasks and the blocks edge', async () => {
      const page = await alice.gui();
      await page.goto(`/items/${initiative}?view=graph`);
      // COLLIERY-T-1836: the Aurora `Dag`; a node's `data-id` is its code.
      await expect(page.locator('.cl-dag__node--current')).toHaveAttribute('data-id', initiative);
      for (const code of [bound, blocker]) {
        await expect(page.locator(`.cl-dag__node[data-id="${code}"]`)).toBeVisible();
      }
      // parent is drawn as containment (lanes); blocks is the one arrow.
      await expect(page.locator('svg.cl-dag path.cl-dag__edge')).toHaveCount(1);
      return { nodes: [initiative, bound, blocker], arrows: 'blocks ×1 (parent shown as containment)' };
    });

    await step(bob, 'opens the platform board and sees the bound task marked blocked', async () => {
      const page = await bob.gui();
      await openBoard(page, 'platform-delivery');
      await expect(cardIn(page, 'Backlog', bound).locator('.cl-pill', { hasText: 'blocked by 1' })).toBeVisible();
      await expect(cardIn(page, 'Backlog', blocker).locator('.cl-pill', { hasText: 'blocks 1' })).toBeVisible();
      return { blocked: bound, blocker };
    });

    await step(alice, 'moves the blocking task to Todo from the CLI', async () => {
      const cli = await alice.cli();
      const api = await alice.api();
      const platform = await api.boardBySlug('platform-delivery');
      const todo = platform.columns.find((c: any) => c.name === 'Todo');
      await cli.ok(['tasks', 'transition', blocker, '--to', todo.id]);
      return { task: blocker, to: 'Todo' };
    });

    await step(bob, 'sees the card arrive in Todo without reloading', async () => {
      const page = await bob.gui();
      await expect(cardIn(page, 'Todo', blocker)).toBeVisible({ timeout: 15_000 });
      await expect(cardIn(page, 'Backlog', blocker)).toHaveCount(0);
      return { task: blocker, now_in: 'Todo', reloaded: false };
    });

    await step(alice, 'traverses from the initiative and filters by repository with `kairos search`', async () => {
      const cli = await alice.cli();
      const children = await cli.json(['search', '--from', initiative, '--relationships', 'parent', '--direction', 'outbound', '--depth', '1']);
      const childCodes = (children.results?.tasks ?? []).map((t: any) => t.short_code).sort();
      expect(childCodes).toEqual([bound, blocker].sort());
      const byRepo = await cli.json(['search', '--repo', 'payments-api', '--limit', '100']);
      const mine = (byRepo.results?.tasks ?? []).map((t: any) => t.short_code).filter((c: string) => c === bound || c === blocker);
      expect(mine).toEqual([bound]);
      return { children: childCodes, payments_api_hits_from_this_run: mine };
    });

    await step(alice, 'checks the plan from the terminal and records the decision behind it as an ADR', async () => {
      const cli = await alice.cli();
      const api = await alice.api();
      const strategyRow = await cli.json(['strategies', 'get', strategy]);
      expect(strategyRow.short_code).toBe(strategy);
      const initiatives = await cli.json(['initiatives', 'list', '--limit', '100']);
      const codes = (initiatives.items ?? initiatives).map((i: any) => i.short_code);
      expect(codes).toContain(initiative);
      // Flight Levels: the decision that shaped this initiative belongs on
      // the record next to it, not in someone's head.
      const adrBoard = await api.boardBySlug('adrs');
      const adr = await cli.json([
        'adrs', 'create', '--board', adrBoard.id,
        '--title', named('decision: CSV before JSON for exports'),
        '--content', 'CSV first: finance already consumes it. JSON when the portal needs it.',
      ]);
      ledger.add({ kind: 'adr', label: adr.short_code, delete: async () => { await api.delete(`/api/adrs/${adr.short_code}`); } });
      return { strategy, initiatives_listed: codes.length, adr: adr.short_code };
    });

    let note = '';
    await step(alice, 'attaches a design note to the initiative from the CLI', async () => {
      const cli = await alice.cli();
      const api = await alice.api();
      const doc = await cli.json(['documents', 'create', '--title', named('design note: export format'), '--board', 'initiatives', '--parent', initiative, '--content', '# Export format\n\nCSV first, JSON later.']);
      note = doc.short_code;
      ledger.add({ kind: 'document', label: note, delete: async () => { await api.delete(`/api/documents/${note}`); } });
      return { document: note, supports: initiative };
    });

    await step(alice, 'moves the design note\'s lifecycle to review in the GUI', async () => {
      const page = await alice.gui();
      await openItem(page, note);
      await expect(page.locator('.kairos-lifecycle-badge')).toContainText('lifecycle: draft');
      const lifecycle = panel(page, 'Lifecycle');
      await lifecycle.locator('select').selectOption({ label: 'review' });
      await lifecycle.getByRole('button', { name: 'Set', exact: true }).click();
      await expect(page.locator('.kairos-lifecycle-badge')).toContainText('lifecycle: review', { timeout: 15_000 });
      return { document: note, lifecycle: 'review' };
    });
  },
);

async function columnOfCard(page: Page, code: string): Promise<string> {
  const col = page.locator('section.kairos-board__column', { has: card(page, code) }).first();
  const head = await col.locator('.kairos-board__column-head').first().innerText();
  return head.trim().split('\n')[0];
}
