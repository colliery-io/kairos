// J17 — "Something breaks on a Friday" (KAIROS-I-0014). The unplanned-work
// path: every other journey walks planned work down a board, so the
// Support lane (KAIROS-T-0077's `work_class` axis) only exists in the e2e
// tier, where nobody is doing anything — a lane is exercised by a drag,
// not by a page load.
//
// carol takes the customer call; bob is on call for the team that owns the
// service; the team's agent lands the fix. What the journey is really
// watching is whether the unplanned item stays distinguishable the whole
// way through: born in the Support lane by a stranger filing against the
// repository, triaged PAST the planned card without disturbing it, flagged
// as support in the agent's queue, and present in the team's in-flight
// rollup — a support item that quietly drops out of the rollup is how an
// incident ends up invisible to everyone but the person holding it.
import { expect, type Locator, type Page } from '@playwright/test';
import { setupTeamRepoAgent, type TeamFixture } from '../fixtures/team';
import { named } from '../run/context';
import { journey, step } from '../run/narrate';
import { createForgeConnection, deliverGithubWebhook, githubPullRequest, type ForgeConnection } from '../surfaces/forge';
import { card, openBoard, openItem, openTeam, panel } from '../surfaces/gui';
import { shortCodes } from '../surfaces/mcp';

// Lane-scoped selectors. `gui.ts`'s `column()` resolves inside the PLANNED
// lane, which is right for every other journey and useless here: this one
// needs to say which lane it means on every assertion, because "the card
// is in Active" is exactly the claim that hides a lane bug.
type Lane = 'planned' | 'support';

function laneColumn(page: Page, lane: Lane, name: string): Locator {
  return page.locator(`section.kairos-board__lane--${lane}`).locator('section.kairos-board__column', {
    has: page.locator('.kairos-board__column-head', { hasText: name }),
  });
}

function laneCard(page: Page, lane: Lane, columnName: string, code: string): Locator {
  return laneColumn(page, lane, columnName).locator('article.kairos-card', { hasText: code });
}

/** Drag a card to a column IN A NAMED LANE (see `gui.dragCard` for why the
 * mouse is driven by hand rather than through `dragTo`). */
async function dragInLane(page: Page, code: string, lane: Lane, toColumn: string): Promise<void> {
  const source = card(page, code).first();
  const target = laneColumn(page, lane, toColumn);
  await target.scrollIntoViewIfNeeded();
  await source.scrollIntoViewIfNeeded();
  const from = await source.boundingBox();
  const box = await target.boundingBox();
  const viewport = page.viewportSize();
  if (!from || !box || !viewport) throw new Error(`no geometry for ${code} → ${lane}/${toColumn}`);
  const at = {
    x: (Math.max(box.x, 0) + Math.min(box.x + box.width, viewport.width)) / 2,
    y: (Math.max(box.y, 0) + Math.min(box.y + box.height, viewport.height)) / 2,
  };
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(at.x, at.y, { steps: 2 });
  await page.mouse.up();
  await expect(laneCard(page, lane, toColumn, code)).toBeVisible({ timeout: 15_000 });
}

journey(
  'incident',
  'Something breaks on a Friday',
  { humans: ['alice', 'bob', 'carol'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    const bob = cast.human('bob');
    const carol = cast.human('carol');
    let team: TeamFixture;
    let connection: ForgeConnection;
    let planned = '';
    let request = '';
    let bug = '';

    await step(alice, 'stands up the on-call team, the service it owns, its agent and a forge webhook (setup)', async () => {
      team = await setupTeamRepoAgent(alice, ledger, 'ops');
      const api = await alice.api();
      const members = (await api.get('/api/members?limit=100')).items ?? [];
      const bobRow = members.find((m: any) => m.email === bob.credentials.email);
      await (await alice.cli()).ok(['teams', 'members', 'add', team.teamId, '--user', bobRow.user_id]);
      connection = await createForgeConnection(api, team.repoSlug);
      ledger.add({
        kind: 'forge-connection',
        label: team.repoSlug,
        delete: async () => { await api.delete(`/api/forge-connections/${connection.id}`); },
      });
      return { team: team.teamSlug, repository: team.repoSlug, board: team.boardSlug, on_call: bob.credentials.email };
    });

    await step(bob, 'starts Friday with planned work: a task in Todo, in the Planned lane', async () => {
      const page = await bob.gui();
      await openBoard(page, team.boardSlug);
      const title = named('task: retry budget for the payments poller');
      await page.getByRole('button', { name: 'New task', exact: true }).click();
      const modal = page.locator('.cl-modal');
      await modal.locator('input.cl-input').first().fill(title);
      await modal.getByRole('button', { name: 'Create' }).click();
      await expect(modal).toBeHidden();
      const created = card(page, title);
      planned = (await created.locator('a.kairos-card__code').innerText()).trim();
      const api = await alice.api();
      ledger.add({ kind: 'task', label: planned, delete: async () => { await api.delete(`/api/tasks/${planned}`); } });
      await dragInLane(page, planned, 'planned', 'Todo');
      return { short_code: planned, lane: 'planned', column: 'Todo' };
    });

    await step(carol, 'is refused when she drops a customer problem straight onto another team\'s board', async () => {
      const mcp = await carol.mcp();
      // A stranger may file into a team's Backlog, but only by naming the
      // codebase it is about (A-0019 §4 `file_backlog`): work with no
      // repository on it is a board write, and that is the team's to make.
      const refusal = await mcp.refused('create_item', {
        item_type: 'task',
        task_type: 'support',
        title: named('support: checkout 500s for trial accounts'),
        board: team.boardSlug,
      });
      expect(refusal).toContain('manage_tasks');
      return { refused: refusal.split('\n')[0].slice(0, 160) };
    });

    await step(carol, 'files it against the service instead; it is born unplanned, in their Support lane', async () => {
      const mcp = await carol.mcp();
      const text = await mcp.call('create_item', {
        item_type: 'task',
        task_type: 'support',
        title: named('support: checkout 500s for trial accounts'),
        repository: team.repoSlug,
        content: 'Three customers on expiring trials cannot check out. Started ~16:40 UTC.',
      });
      [request] = shortCodes(text);
      const api = await alice.api();
      ledger.add({ kind: 'task', label: request, delete: async () => { await api.delete(`/api/tasks/${request}`); } });
      const item = await mcp.call('get_item', { short_code: request });
      // A support-type ticket is born in the Support lane wherever it comes
      // from — the default has to survive the cross-team filing path too.
      expect(item).toContain('lane: support');
      const boardLine = item.match(/- board: ([^\n]+)/)?.[1];
      expect(boardLine).toContain(team.boardSlug);
      expect(boardLine).toContain('Backlog');
      return { short_code: request, lane: 'support', landed_on: boardLine };
    });

    await step(bob, 'triages it past his planned work: Support/Backlog → Todo → Active, and the planned card does not move', async () => {
      const page = await bob.gui();
      await openBoard(page, team.boardSlug);
      const lanes = page.locator('section.kairos-board__lane');
      await expect(lanes).toHaveCount(2);
      // Unplanned work reads above planned work, which is the whole point
      // of having two lanes rather than a label.
      await expect(lanes.first()).toHaveClass(/kairos-board__lane--support/);
      await expect(laneCard(page, 'support', 'Backlog', request)).toBeVisible();
      await dragInLane(page, request, 'support', 'Todo');
      await dragInLane(page, request, 'support', 'Active');
      await expect(laneCard(page, 'planned', 'Todo', planned)).toBeVisible();
      await expect(laneCard(page, 'support', 'Active', planned)).toHaveCount(0);
      return { support: `${request} → Active`, planned_still_in: 'Todo', lane_order: 'support above planned' };
    });

    await step(bob, 'traces it to the code and raises a bug from the request, in the same lane', async () => {
      const mcp = await bob.mcp();
      // The product has no way to RETYPE a request as a bug (task_type is
      // set at creation and there is no endpoint for it), so the bug is
      // raised from the request and linked — which is also the honest
      // record: the customer's report and the defect are two facts.
      const text = await mcp.call('create_item', {
        item_type: 'task',
        task_type: 'bug',
        work_class: 'support',
        title: named('bug: trial expiry check panics on a null plan'),
        repository: team.repoSlug,
        content: 'Null plan on an expired trial reaches the checkout handler and panics.',
      });
      [bug] = shortCodes(text);
      const api = await alice.api();
      ledger.add({ kind: 'task', label: bug, delete: async () => { await api.delete(`/api/tasks/${bug}`); } });
      await mcp.call('link_items', { source: bug, target: request, relationship: 'blocks' });
      const item = await mcp.call('get_item', { short_code: bug });
      expect(item).toContain('lane: support');
      expect(item).toContain(`repository: ${team.repoSlug} (owner: ${team.teamSlug})`);
      return { bug, lane: 'support', repository: team.repoSlug, edge: `${bug} blocks ${request}` };
    });

    const agent = cast.agent('agent', team!.apiKey);

    await step(agent, 'finds the bug in its repository queue, flagged as unplanned, and picks it up', async () => {
      const mcp = await agent.mcp();
      const queue = await mcp.call('board_items', { board: team.boardSlug, repository: team.repoSlug });
      expect(shortCodes(queue)).toContain(bug);
      // The lane reaches the machine surface too: an agent that cannot see
      // which of its tickets is an incident will work them in filed order.
      const line = queue.split('\n').find((l) => l.includes(bug)) ?? '';
      expect(line).toContain('bug [support lane]');
      await mcp.call('transition_item', { short_code: bug, to_column: 'Todo' });
      await mcp.call('transition_item', { short_code: bug, to_column: 'Active' });
      return { queue_line: line.trim().slice(0, 120), moved_to: 'Active' };
    });

    await step(agent, 'opens a pull request naming the bug; the fix links back to the ticket', async () => {
      const opened = githubPullRequest({
        number: 31,
        code: bug,
        repoFullName: team.repoFullName,
        state: 'open',
        updatedAt: '2026-09-23T17:05:00Z',
        title: `Guard the null plan (${bug})`,
      });
      expect(await deliverGithubWebhook(connection, 'pull_request', opened)).toBe(200);
      const mcp = await agent.mcp();
      const repo = await mcp.call('get_repository', { repository: team.repoSlug });
      expect(repo).toContain('pull_request 31 [open]');
      expect(repo).toContain(bug);
      return { pr: '#31', state: 'open', linked_to: bug };
    });

    await step(bob, 'sees Friday\'s unplanned work in the team\'s in-flight rollup, not just on the lane', async () => {
      const page = await bob.gui();
      await openTeam(page, team.teamSlug);
      const inFlight = panel(page, 'In flight');
      // The catch: a support item that reaches the lane but never the
      // rollup is invisible to everyone who reads the team page instead of
      // the board — which is everyone above the team.
      // The rollup prints the PR and the item it hangs off; assert the link
      // back to the ticket, which is the one a reader would follow.
      await expect(inFlight.locator(`a[href="/items/${bug}"]`)).toBeVisible({ timeout: 20_000 });
      await expect(inFlight.getByText('#31', { exact: false })).toBeVisible();
      return { rollup: 'In flight', names: bug, pr: '#31' };
    });

    await step(agent, 'lands the fix and closes the bug', async () => {
      const merged = githubPullRequest({
        number: 31,
        code: bug,
        repoFullName: team.repoFullName,
        state: 'closed',
        merged: true,
        updatedAt: '2026-09-23T17:40:00Z',
        title: `Guard the null plan (${bug})`,
      });
      expect(await deliverGithubWebhook(connection, 'pull_request', merged)).toBe(200);
      const mcp = await agent.mcp();
      const item = await mcp.call('get_item', { short_code: bug });
      const prLine = item.split('## Development')[1]?.split('\n').find((l) => l.includes('pull_request 31'));
      expect(prLine).toContain('[merged]');
      await mcp.call('transition_item', { short_code: bug, to_column: 'Completed' });
      return { pr: '#31', development_line: prLine?.trim(), moved_to: 'Completed' };
    });

    await step(bob, 'tells the customer it is fixed and closes the request; the Support lane empties', async () => {
      const page = await bob.gui();
      await openItem(page, request);
      await expect(panel(page, 'Relationships').getByText(bug)).toBeVisible();
      await openBoard(page, team.boardSlug);
      await dragInLane(page, request, 'support', 'Completed');
      await expect(laneCard(page, 'support', 'Active', request)).toHaveCount(0);
      await expect(laneCard(page, 'planned', 'Todo', planned)).toBeVisible();
      return { request, column: 'Completed', planned_work_resumes: planned };
    });

    await step(alice, 'counts what Friday cost: the unplanned work separates cleanly from the planned', async () => {
      const cli = await alice.cli();
      const unplanned = await cli.json(['search', '--work-class', 'support', '--repo', team.repoSlug, '--limit', '100']);
      const unplannedCodes = (unplanned.results?.tasks ?? []).map((t: any) => t.short_code);
      expect(unplannedCodes).toContain(request);
      expect(unplannedCodes).toContain(bug);
      expect(unplannedCodes).not.toContain(planned);
      const asPlanned = await cli.json(['search', '--work-class', 'planned', '--board', team.boardId, '--limit', '100']);
      const plannedCodes = (asPlanned.results?.tasks ?? []).map((t: any) => t.short_code);
      expect(plannedCodes).toContain(planned);
      expect(plannedCodes).not.toContain(request);
      return { unplanned: unplannedCodes.length, unplanned_codes: [request, bug], planned_codes: [planned] };
    });
  },
);
