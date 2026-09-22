// J4 — "A web engineer needs something from platform and gets it"
// (KAIROS-I-0011 D4). carol (web team) works as her own agent over MCP and
// follows the plugin's CROSS-TEAM-FILING recipe to the letter:
// list_repositories → get_repository → create_item against platform's
// repo with no board → link_items blocks. Everything else is exactly what
// the recipe says she cannot do — and the product refuses. bob triages on
// the platform board in the GUI; carol watches her own board live.
//
// A `blocks` badge counts LIVE edges, not the blocker's column, so the
// dependency clears when carol removes the edge once platform is done —
// the edge is hers to remove because she authored its source.
import { expect } from '@playwright/test';
import { named } from '../run/context';
import { journey, step } from '../run/narrate';
import { cardIn, column, dragCard, openBoard, openItem, panel } from '../surfaces/gui';
import { shortCodes } from '../surfaces/mcp';

const THEIR_REPO = process.env.UAT_THEIR_REPO ?? 'payments-api';
const MY_REPO = process.env.UAT_MY_REPO ?? 'portal-web';

journey(
  'cross-team',
  'A web engineer needs something from platform and gets it',
  { humans: ['alice', 'bob', 'carol'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    const bob = cast.human('bob');
    const carol = cast.human('carol');
    let theirBoard = '';
    let filed = '';
    let mine = '';

    await step(carol, `reads platform's repository over MCP: who owns ${THEIR_REPO} and how they work`, async () => {
      const mcp = await carol.mcp();
      const directory = await mcp.call('list_repositories');
      expect(directory).toContain(THEIR_REPO);
      const repo = await mcp.call('get_repository', { repository: THEIR_REPO });
      const owner = repo.match(/- owner team: ([a-z0-9-]+)/)?.[1];
      // The delivery board is printed as a slug — what board_items takes.
      theirBoard = repo.match(/- delivery board: ([a-z0-9][a-z0-9-]*) \(/)?.[1] ?? '';
      expect(owner).toBeTruthy();
      expect(theirBoard).toBeTruthy();
      const howToWorkHere = repo.split('## How to work here')[1]?.split('##')[0]?.trim().split('\n')[0];
      return { repository: THEIR_REPO, owner, their_board: theirBoard, how_to_work_here: howToWorkHere?.slice(0, 80) };
    });

    await step(carol, `files a task against ${THEIR_REPO} with no board; it lands in platform's Backlog`, async () => {
      const mcp = await carol.mcp();
      const text = await mcp.call('create_item', {
        item_type: 'task',
        title: named('platform: bulk invoice export endpoint'),
        repository: THEIR_REPO,
        content: 'Portal needs a bulk export of invoices (CSV) for the finance page. Done = endpoint documented and deployed to staging.',
      });
      [filed] = shortCodes(text);
      expect(filed).toBeTruthy();
      const api = await alice.api();
      ledger.add({ kind: 'task', label: filed, delete: async () => { await api.delete(`/api/tasks/${filed}`); } });
      const item = await mcp.call('get_item', { short_code: filed });
      const boardLine = item.match(/- board: ([^\n]+)/)?.[1];
      expect(boardLine).toContain(theirBoard);
      expect(boardLine).toContain('Backlog');
      return { short_code: filed, landed_on: boardLine };
    });

    await step(carol, `creates her own task on ${MY_REPO} and links the platform task as blocking it`, async () => {
      const mcp = await carol.mcp();
      const text = await mcp.call('create_item', {
        item_type: 'task',
        title: named('portal: finance page bulk export button'),
        repository: MY_REPO,
      });
      [mine] = shortCodes(text);
      const api = await alice.api();
      ledger.add({ kind: 'task', label: mine, delete: async () => { await api.delete(`/api/tasks/${mine}`); } });
      const linked = await mcp.call('link_items', { source: filed, target: mine, relationship: 'blocks' });
      return { my_task: mine, edge: `${filed} blocks ${mine}`, tool_said: linked.split('\n')[0] };
    });

    await step(carol, 'is refused when she tries to move the platform task out of their Backlog', async () => {
      const mcp = await carol.mcp();
      const refusal = await mcp.refused('transition_item', { short_code: filed, to_column: 'Todo' });
      // The refusal teaches the rule the recipe describes, not just a capability name.
      expect(refusal).toContain('Backlog for their triage');
      expect(refusal).toContain('file_backlog');
      return { refused: refusal.split('\n')[0].slice(0, 160) };
    });

    await step(bob, 'sees the filed card in platform Backlog with its repo chip and triages it to Todo', async () => {
      const page = await bob.gui();
      await openBoard(page, theirBoard);
      const card = cardIn(page, 'Backlog', filed);
      await expect(card).toBeVisible();
      await expect(card.locator(`.kairos-card__repo[data-repo="${THEIR_REPO}"]`)).toBeVisible();
      await expect(card.locator('.cl-pill', { hasText: 'blocks 1' })).toBeVisible();
      await dragCard(page, filed, 'Todo');
      return { task: filed, chip: THEIR_REPO, column: 'Todo' };
    });

    await step(carol, 'sees her own task marked blocked by the platform task', async () => {
      const page = await carol.gui();
      const myBoard = (await (await alice.api()).task(mine)).board_id;
      const slug = (await (await alice.api()).get(`/api/boards/${myBoard}`)).slug;
      await openBoard(page, slug);
      await expect(cardIn(page, 'Backlog', mine).locator('.cl-pill', { hasText: 'blocked by 1' })).toBeVisible();
      await openItem(page, mine);
      await expect(panel(page, 'Relationships').getByText(filed)).toBeVisible();
      await openBoard(page, slug);
      return { my_task: mine, badge: 'blocked by 1', names: filed };
    });

    await step(bob, 'takes the platform task through to Completed', async () => {
      const page = await bob.gui();
      await openBoard(page, theirBoard);
      await dragCard(page, filed, 'Active');
      await dragCard(page, filed, 'Completed');
      return { task: filed, column: 'Completed' };
    });

    await step(carol, 'removes the dependency now platform is done; her badge clears without a reload', async () => {
      const mcp = await carol.mcp();
      const platformItem = await mcp.call('get_item', { short_code: filed });
      expect(platformItem).toContain('column: Completed');
      await mcp.call('unlink_items', { source: filed, target: mine, relationship: 'blocks' });
      const page = await carol.gui();
      await expect(cardIn(page, 'Backlog', mine).locator('.cl-pill', { hasText: 'blocked by' })).toHaveCount(0, { timeout: 15_000 });
      return { my_task: mine, badge: 'cleared', reloaded: false };
    });

    await step(alice, `lists what was filed against ${THEIR_REPO} with the CLI and sees carol as the author`, async () => {
      const cli = await alice.cli();
      const hits = await cli.json(['search', '--repo', THEIR_REPO, '--limit', '100']);
      const row = (hits.results?.tasks ?? []).find((t: any) => t.short_code === filed);
      expect(row).toBeTruthy();
      const me = await (await carol.api()).whoami();
      expect(row.created_by).toBe(me.user.id);
      return { task: filed, created_by: me.user.email };
    });
  },
);
