// J3 — "An agent picks up a ticket in its repository and lands a PR"
// (KAIROS-I-0011 D4). The agent persona is a service account on bob's
// team (UAT_TEAM, default `platform`), which registers one repository for
// this run — the J1 fixture minus the team, because a team whose board has
// held an item can never be deleted and this journey must clean up after
// itself. The agent makes exactly the MCP/CLI calls the plugin's
// `implement` skill documents: whoami →
// list_repositories → get_repository → board_items narrowed to the repo →
// get_item → transition_item. bob, a human on the team, raises the ticket
// in the GUI and watches the PR arrive; the forge speaks through signed
// webhooks on a connection alice creates for the repo.
import { expect } from '@playwright/test';
import { setupRepoAgentOnTeam, type TeamFixture } from '../fixtures/team';
import { named } from '../run/context';
import { journey, step } from '../run/narrate';
import { createForgeConnection, deliverGithubWebhook, githubPullRequest, type ForgeConnection } from '../surfaces/forge';
import { card, cardIn, dragCard, openBoard, openItem, panel } from '../surfaces/gui';
import { shortCodes } from '../surfaces/mcp';

journey(
  'agent-loop',
  'An agent picks up a ticket in its repository and lands a PR',
  { humans: ['alice', 'bob'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    const bob = cast.human('bob');
    let team: TeamFixture;
    let connection: ForgeConnection;
    let code = '';

    await step(alice, "registers this run's repository on bob's team, with a coding agent and a forge webhook (setup)", async () => {
      team = await setupRepoAgentOnTeam(alice, ledger, 'mobile');
      const api = await alice.api();
      const me = await (await bob.api()).whoami();
      expect((me.teams as any[]).map((t) => t.slug)).toContain(team.teamSlug);
      connection = await createForgeConnection(api, team.repoSlug);
      ledger.add({ kind: 'forge-connection', label: team.repoSlug, delete: async () => { await api.delete(`/api/forge-connections/${connection.id}`); } });
      return { team: team.teamSlug, repository: team.repoSlug, board: team.boardSlug, webhook: new URL(connection.webhookUrl).pathname };
    });

    const agent = cast.agent('agent', team!.apiKey);

    await step(agent, 'bootstraps over MCP: whoami → list_repositories → get_repository, reads how to work here', async () => {
      const mcp = await agent.mcp();
      const me = await mcp.call('whoami');
      expect(me).toContain(`- ${team.teamSlug} —`);
      const list = await mcp.call('list_repositories', { team: team.teamSlug });
      expect(list).toContain(team.repoSlug);
      const repo = await mcp.call('get_repository', { repository: team.repoSlug });
      expect(repo).toContain('flutter test');
      expect(repo).toContain('(nothing open)');
      return { repository: team.repoSlug, how_to_work_here: 'read', in_flight: 'nothing open' };
    });

    await step(bob, 'raises a task on the team board, binds it to the repository and moves it to Todo', async () => {
      const page = await bob.gui();
      await openBoard(page, team.boardSlug);
      const title = named('task: offline receipts cache');
      await page.getByRole('button', { name: 'New task', exact: true }).click();
      const modal = page.locator('.cl-modal');
      await modal.locator('input.cl-input').first().fill(title);
      // The New task modal offers the team's repositories.
      await modal.locator('[data-testid="create-repository"] select').selectOption(team.repoSlug);
      await modal.getByRole('button', { name: 'Create' }).click();
      const created = card(page, title);
      await expect(created.locator('a.kairos-card__code')).toHaveText(/-T-\d{4}/);
      code = (await created.locator('a.kairos-card__code').innerText()).trim();
      const api = await alice.api();
      ledger.add({ kind: 'task', label: code, delete: async () => { await api.delete(`/api/tasks/${code}`); } });
      await expect(created.locator(`.kairos-card__repo[data-repo="${team.repoSlug}"]`)).toBeVisible();
      await dragCard(page, code, 'Todo');
      return { short_code: code, repository: team.repoSlug, column: 'Todo' };
    });

    await step(agent, 'finds exactly that task in its repository queue and takes it to Active', async () => {
      const mcp = await agent.mcp();
      const queue = await mcp.call('board_items', { board: team.boardSlug, column: 'Todo', repository: team.repoSlug });
      expect(shortCodes(queue)).toEqual([code]);
      const item = await mcp.call('get_item', { short_code: code });
      // get_item prints `… · repository: <slug> (owner: <team>)` on the type line.
      const repositoryLine = item.match(/repository: ([^\n·]+)/)?.[1]?.trim();
      expect(repositoryLine).toBe(`${team.repoSlug} (owner: ${team.teamSlug})`);
      await mcp.call('transition_item', { short_code: code, to_column: 'Active' });
      return { queue: shortCodes(queue), repository_line: repositoryLine, moved_to: 'Active' };
    });

    await step(agent, 'confirms the state from the CLI: the task is Active and the repo shows it as open work', async () => {
      const cli = await agent.cli();
      const task = await cli.json(['tasks', 'get', code]);
      const api = await alice.api();
      const board = await api.get(`/api/boards/${team.boardId}`);
      const columnName = board.columns.find((c: any) => c.id === task.column_id)?.name;
      expect(columnName).toBe('Active');
      const repo = await cli.json(['repos', 'get', team.repoSlug]);
      expect(repo.open_tasks).toBe(1);
      return { task: code, column: columnName, open_tasks_on_repo: repo.open_tasks };
    });

    await step(agent, 'opens a pull request naming the task; Kairos links it to the ticket', async () => {
      const opened = githubPullRequest({
        number: 7,
        code,
        repoFullName: team.repoFullName,
        state: 'open',
        updatedAt: '2026-09-22T12:00:00Z',
        title: `Offline receipts cache (${code})`,
      });
      expect(await deliverGithubWebhook(connection, 'pull_request', opened)).toBe(200);
      const mcp = await agent.mcp();
      const repo = await mcp.call('get_repository', { repository: team.repoSlug });
      expect(repo).toContain(`pull_request 7 [open]`);
      expect(repo).toContain(code);
      return { pr: '#7', state: 'open', linked_to: code };
    });

    await step(bob, 'sees the pull request in the task\'s Development panel', async () => {
      const page = await bob.gui();
      await openItem(page, code);
      const dev = panel(page, 'Development');
      await expect(dev.getByText('#7', { exact: false })).toBeVisible({ timeout: 20_000 });
      await expect(dev.getByText(team.repoFullName, { exact: false })).toBeVisible();
      return { panel: 'Development', pr: '#7' };
    });

    await step(agent, 'has the PR merged; the link flips to merged and the task is Completed', async () => {
      const merged = githubPullRequest({
        number: 7,
        code,
        repoFullName: team.repoFullName,
        state: 'closed',
        merged: true,
        updatedAt: '2026-09-22T12:30:00Z',
        title: `Offline receipts cache (${code})`,
      });
      expect(await deliverGithubWebhook(connection, 'pull_request', merged)).toBe(200);
      const mcp = await agent.mcp();
      // The ticket itself now shows the PR and its state (## Development);
      // the repository's in-flight list no longer lists it.
      const item = await mcp.call('get_item', { short_code: code });
      const prLine = item.split('## Development')[1]?.split('\n').find((l) => l.includes('pull_request 7'));
      expect(prLine).toContain('[merged]');
      const repo = await mcp.call('get_repository', { repository: team.repoSlug });
      expect(repo).toContain('(nothing open)');
      await mcp.call('transition_item', { short_code: code, to_column: 'Completed' });
      return { pr: '#7', development_line: prLine?.trim(), moved_to: 'Completed', in_flight: 'nothing open' };
    });

    await step(bob, 'sees the merged chip on the task and the card in Completed', async () => {
      const page = await bob.gui();
      const row = panel(page, 'Development').locator('.cl-group').filter({ hasText: '#7' }).first();
      await expect(row.locator('.cl-pill', { hasText: 'merged' })).toBeVisible({ timeout: 20_000 });
      await openBoard(page, team.boardSlug);
      await expect(cardIn(page, 'Completed', code)).toBeVisible();
      return { task: code, column: 'Completed', pr_chip: 'merged' };
    });
  },
);
