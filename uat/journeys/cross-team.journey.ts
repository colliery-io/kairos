// J4 — "A web engineer needs something from platform and gets it"
// (KAIROS-I-0011 D4). carol (web team) works as her own agent over MCP and
// follows the plugin's CROSS-TEAM-FILING recipe to the letter:
// list_repositories → get_repository → create_item on platform's board
// with platform's repo → link_items blocks. She names the board: a
// repository is a link and does not choose one (COLLIERY-T-0217).
//
// What she sends is a REQUEST (COLLIERY-T-0218, COLLIERY-A-0023): teams
// request work of each other, and no team pushes work to a different team.
// The request goes to the entry column of platform's board, in the Support
// lane. She cannot select the planned lane. The repository is optional; she
// names it here because she knows it.
//
// She created the request, so she can edit it, link it and archive it (the
// edit rule and the link rule, COLLIERY-T-0228). She cannot MOVE it: not
// to another column, not to the planned lane, not to another board. That
// is what the recipe says she cannot do — and the product refuses. bob
// triages on the platform board in the GUI; carol watches her own board
// live.
//
// A `blocks` badge counts LIVE edges, not the blocker's column, so the
// dependency clears when carol removes the edge once platform is done —
// the edge is hers to remove because she can edit the item at one end.
import { expect } from '@playwright/test';
import { named } from '../run/context';
import { journey, step } from '../run/narrate';
import { card, cardIn, dragCard, laneCard, openBoard, openItem, panel } from '../surfaces/gui';
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
    let carolBoard = '';
    let filed = '';
    let mine = '';

    await step(carol, `reads platform's repository over MCP: who owns ${THEIR_REPO} and how they work`, async () => {
      const mcp = await carol.mcp();
      const directory = await mcp.call('list_repositories');
      expect(directory).toContain(THEIR_REPO);
      const repo = await mcp.call('get_repository', { repository: THEIR_REPO });
      const owner = repo.match(/- owner team: ([a-z0-9-]+)/)?.[1];
      // The delivery board is printed as a slug — what board_items takes.
      theirBoard = repo.match(/- owner's delivery board: ([a-z0-9][a-z0-9-]*) \(/)?.[1] ?? '';
      expect(owner).toBeTruthy();
      expect(theirBoard).toBeTruthy();
      const howToWorkHere = repo.split('## How to work here')[1]?.split('##')[0]?.trim().split('\n')[0];
      return { repository: THEIR_REPO, owner, their_board: theirBoard, how_to_work_here: howToWorkHere?.slice(0, 80) };
    });

    await step(carol, 'is refused when she asks for the planned lane of platform\'s board', async () => {
      const mcp = await carol.mcp();
      const refusal = await mcp.refused('create_item', {
        item_type: 'task',
        work_class: 'planned',
        title: named('platform: bulk invoice export endpoint'),
        board: theirBoard,
        repository: THEIR_REPO,
      });
      // The planned lane is platform's own plan. A request does not go there.
      expect(refusal).toContain('FORBIDDEN');
      expect(refusal).toContain('support lane');
      expect(refusal).toContain('manage_tasks');
      return { refused: refusal.split('\n')[0].slice(0, 160) };
    });

    await step(carol, `sends a request to platform, linked to ${THEIR_REPO}; it lands in the entry column, in the Support lane`, async () => {
      const mcp = await carol.mcp();
      const text = await mcp.call('create_item', {
        item_type: 'task',
        title: named('platform: bulk invoice export endpoint'),
        board: theirBoard,
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
      expect(boardLine).toContain('column: Backlog');
      // She sent no work class, and the type is `task`. Until
      // COLLIERY-T-0218 that was the planned lane.
      expect(item).toContain('(task) · lane: support');
      return { short_code: filed, lane: 'support', landed_on: boardLine };
    });

    await step(carol, `creates her own task on ${MY_REPO} and links the platform task as blocking it`, async () => {
      const mcp = await carol.mcp();
      const myRepo = await mcp.call('get_repository', { repository: MY_REPO });
      const myBoard = myRepo.match(/- owner's delivery board: ([a-z0-9][a-z0-9-]*) \(/)?.[1] ?? '';
      expect(myBoard).toBeTruthy();
      const text = await mcp.call('create_item', {
        item_type: 'task',
        title: named('portal: finance page bulk export button'),
        board: myBoard,
        repository: MY_REPO,
      });
      [mine] = shortCodes(text);
      const api = await alice.api();
      ledger.add({ kind: 'task', label: mine, delete: async () => { await api.delete(`/api/tasks/${mine}`); } });
      const linked = await mcp.call('link_items', { source: filed, target: mine, relationship: 'blocks' });
      return { my_task: mine, edge: `${filed} blocks ${mine}`, tool_said: linked.split('\n')[0] };
    });

    await step(carol, 'is refused when she tries to move her request out of the entry column', async () => {
      const mcp = await carol.mcp();
      const refusal = await mcp.refused('transition_item', { short_code: filed, to_column: 'Todo' });
      // The refusal teaches the rule, not just a capability name: she filed
      // the request, it is in the entry column, and the team moves it.
      expect(refusal).toContain('is a request in the entry column');
      expect(refusal).toContain('That team moves it');
      expect(refusal).toContain('file_backlog');
      expect(refusal).toContain('transition_items');
      // COLLIERY-T-0228: it also says what she CAN do, and it is true (the
      // next step). Until then it said "To move, edit or delete it".
      expect(refusal).toContain('you can edit it, link it and archive it');
      expect(refusal).not.toContain('To move, edit or delete it');
      return { refused: refusal.split('\n')[0].slice(0, 200) };
    });

    await step(carol, 'corrects the text of her request: she created it, so she can edit it', async () => {
      // The edit rule (COLLIERY-T-0228). She holds nothing on platform's
      // board. Until then this call was refused, and a wrong word in a
      // request stayed wrong until platform had time for it.
      const mcp = await carol.mcp();
      const edited = await mcp.call('edit_item', {
        short_code: filed,
        search: '(CSV)',
        replace: '(CSV, UTF-8)',
      });
      expect(edited).toContain(`Kairos changed the text of ${filed}`);
      const item = await mcp.call('get_item', { short_code: filed });
      expect(item).toContain('(CSV, UTF-8)');
      // An edit moves nothing: the entry column, the Support lane.
      expect(item).toContain('column: Backlog');
      expect(item).toContain('(task) · lane: support');
      return { edited: filed, still_in: 'Backlog, support lane' };
    });

    await step(carol, 'opens her request in the GUI: the edit controls are there, the move controls are not', async () => {
      // The page mirrors the server (COLLIERY-T-0228). She created the
      // request, so she gets the controls that edit it. She holds nothing
      // on platform's board, so she gets no control that moves it.
      const page = await carol.gui();
      await openItem(page, filed);
      const repository = page.locator('[data-testid="repository-control"]');
      await expect(repository, 'the repository is an edit').toBeVisible({ timeout: 10_000 });
      await expect(repository.getByRole('button', { name: 'Set repository' })).toBeVisible();
      await expect(page.getByRole('button', { name: 'Delete' })).toBeEnabled();
      // The repository control needs whoami, so whoami has resolved: the
      // move controls are absent because she may not move, not because the
      // page is still loading.
      await expect(page.locator('.kairos-item__move'), 'no column move, no lane change').toHaveCount(0);
      await expect(page.locator('[data-testid="move-board"]'), 'no board move').toHaveCount(0);
      return { shown: 'repository, delete, editor', hidden: 'move, lane, board' };
    });

    await step(carol, 'sends a second request from platform\'s board in the GUI; it lands in the entry column, in the Support lane', async () => {
      // COLLIERY-T-0232. Until then the board of a team that was not hers
      // had no create button for her, and a request needed MCP or the API.
      // She does not manage the board, so the button is "New request" and
      // the dialog has no lane to select.
      const page = await carol.gui();
      await openBoard(page, theirBoard);
      const button = page.getByRole('button', { name: 'New request', exact: true });
      await expect(button).toBeVisible({ timeout: 10_000 });
      await expect(page.getByRole('button', { name: 'New task', exact: true })).toHaveCount(0);
      await button.click();
      const modal = page.locator('.cl-modal');
      await expect(modal.locator('.cl-modal__title')).toHaveText(/^Request to /);
      await expect(modal.locator('[data-testid="create-caption"]')).toContainText('in the Support lane');
      const lane = modal.locator('.cl-field', { has: page.locator('.cl-field__label', { hasText: 'Lane' }) });
      await expect(lane, 'a request has no lane to select').toHaveCount(0);
      const title = named('platform: the export needs a currency column');
      await modal
        .locator('.cl-field', { has: page.locator('.cl-field__label', { hasText: 'Title' }) })
        .locator('input')
        .fill(title);
      await modal.getByRole('button', { name: 'Send request', exact: true }).click();
      await expect(modal).toBeHidden({ timeout: 10_000 });
      const notice = page.locator('[data-testid="request-sent"]');
      await expect(notice).toContainText('in the Support lane');
      const second = (await notice.locator('a').innerText()).trim();
      expect(second).toBeTruthy();
      const api = await alice.api();
      ledger.add({ kind: 'task', label: second, delete: async () => { await api.delete(`/api/tasks/${second}`); } });
      await expect(laneCard(page, 'support', 'Backlog', second)).toBeVisible({ timeout: 10_000 });
      await expect(cardIn(page, 'Backlog', second), 'it is not in the planned lane').toHaveCount(0);
      const stored = await api.task(second);
      expect(stored.work_class).toBe('support');
      return { short_code: second, sent_from: 'GUI', lane: 'support', column: 'Backlog' };
    });

    await step(bob, 'sees the request in the Support lane of platform\'s Backlog, takes it into the plan, and triages it to Todo', async () => {
      const page = await bob.gui();
      await openBoard(page, theirBoard);
      const requested = laneCard(page, 'support', 'Backlog', filed);
      await expect(requested).toBeVisible();
      await expect(cardIn(page, 'Backlog', filed), 'it is not in the planned lane').toHaveCount(0);
      await expect(requested.locator(`.kairos-card__repo[data-repo="${THEIR_REPO}"]`)).toBeVisible();
      await expect(requested.locator('.cl-pill', { hasText: 'blocks 1' })).toBeVisible();
      // The team plans its own work: bob, a member of the receiving team,
      // changes the work class. carol could not.
      const api = await bob.api();
      const planned = await api.post(`/api/tasks/${filed}/work-class`, { work_class: 'planned' });
      expect(planned.work_class).toBe('planned');
      await page.reload();
      await expect(cardIn(page, 'Backlog', filed)).toBeVisible({ timeout: 15_000 });
      await dragCard(page, filed, 'Todo');
      return { task: filed, chip: THEIR_REPO, lane: 'support → planned', column: 'Todo' };
    });

    await step(carol, 'sees her own task marked blocked by the platform task', async () => {
      const page = await carol.gui();
      const myBoard = (await (await alice.api()).task(mine)).board_id;
      carolBoard = (await (await alice.api()).get(`/api/boards/${myBoard}`)).slug;
      await openBoard(page, carolBoard);
      await expect(cardIn(page, 'Backlog', mine).locator('.cl-pill', { hasText: 'blocked by 1' })).toBeVisible();
      await openItem(page, mine);
      await expect(panel(page, 'Relationships').getByText(filed)).toBeVisible();
      await openBoard(page, carolBoard);
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

    // The seam has a second shape: work that landed on the wrong side of it
    // is MOVED, not recreated (KAIROS-I-0012).
    await step(bob, 'has no board to move it to — the control is not offered to a one-team member', async () => {
      const page = await bob.gui();
      await openItem(page, filed);
      // Two-sided: bob manages platform only, so there is nowhere he may
      // push this card. The GUI hides the control rather than dangling it.
      await expect(page.locator('[data-testid="move-board"]')).toHaveCount(0);
      return { board_select_offered: false };
    });

    await step(alice, `moves the ticket to the web board while it is still bound to ${THEIR_REPO}, and it keeps the repository`, async () => {
      const page = await alice.gui();
      await openItem(page, filed);
      const control = page.locator('[data-testid="move-board"]');
      await expect(control).toBeVisible();
      await control.locator('select').selectOption(carolBoard);
      await control.getByRole('button', { name: 'Move board' }).click();
      // Until COLLIERY-T-0217 this was refused with REPOSITORY_OWNER_MISMATCH
      // and the ticket had to be unbound first. A move does not look at the
      // repository now.
      await expect(page.getByText(/Moved to /)).toBeVisible({ timeout: 10_000 });
      const moved = await (await alice.api()).task(filed);
      expect(moved.repository?.slug, 'the ticket keeps its repository').toBe(THEIR_REPO);
      await openBoard(page, theirBoard);
      await expect(card(page, filed)).toHaveCount(0);
      return { task: filed, still_bound_to: THEIR_REPO, moved_to: carolBoard };
    });

    await step(carol, 'finds it on her board but cannot push it back — platform\'s board is not hers to write', async () => {
      const mcp = await carol.mcp();
      const onMyBoard = await mcp.call('board_items', { board: carolBoard });
      expect(shortCodes(onMyBoard)).toContain(filed);
      const refusal = await mcp.refused('move_item', { short_code: filed, to_board: theirBoard });
      expect(refusal).toContain('FORBIDDEN');
      return { found_on: carolBoard, refused: refusal.split('\n')[0].slice(0, 150) };
    });

    await step(alice, 'moves it back as org admin, and it lands in the entry column', async () => {
      const mcp = await alice.mcp();
      const text = await mcp.call('move_item', { short_code: filed, to_board: theirBoard });
      expect(text).toContain(`Kairos moved ${filed}`);
      expect(text).toContain(theirBoard);
      const item = await mcp.call('get_item', { short_code: filed });
      expect(item).toContain(`board: ${theirBoard}`);
      expect(item).toContain('Backlog');
      return { tool_said: text.trim().slice(0, 140) };
    });
  },
);
