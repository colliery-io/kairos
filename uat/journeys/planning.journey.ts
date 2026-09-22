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
      const graphTab = page.getByRole('link', { name: 'Graph', exact: true });
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

    await step(alice, 'decomposes it from the CLI into two platform tasks, one bound to payments-api, the other blocking it', async () => {
      const cli = await alice.cli();
      const api = await alice.api();
      const platform = await api.boardBySlug('platform-delivery');
      const t1 = await cli.json(['tasks', 'create', '--repo', 'payments-api', '--title', named('task: export endpoint')]);
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
      await expect(page.locator('.kairos-graph__node--focus .kairos-graph__code')).toHaveText(initiative);
      for (const code of [bound, blocker]) {
        await expect(page.locator('.kairos-graph__node', { has: page.locator('.kairos-graph__code', { hasText: code }) })).toBeVisible();
      }
      // parent is drawn as containment (lanes); blocks is the one arrow.
      await expect(page.locator('.kairos-graph__edge')).toHaveCount(1);
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

    let note = '';
    await step(alice, 'attaches a design note to the initiative from the CLI', async () => {
      const cli = await alice.cli();
      const api = await alice.api();
      const doc = await cli.json(['documents', 'create', '--title', named('design note: export format'), '--parent', initiative, '--content', '# Export format\n\nCSV first, JSON later.']);
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
