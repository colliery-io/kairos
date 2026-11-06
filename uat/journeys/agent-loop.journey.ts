// J3 — "An agent picks up a ticket in its repository and lands a PR"
// (KAIROS-I-0011 D4). The agent persona is a service account on a team
// created for this run, which owns one repository — the whole J1 fixture.
// (It ran on an existing team until KAIROS-I-0012 made a team deletable
// again once its board holds no live cards; now the ledger deletes the
// task and the team goes with it.) The agent makes exactly the MCP/CLI
// calls the plugin's `implement` skill documents: whoami →
// list_repositories → get_repository → board_items narrowed to the repo →
// get_item → transition_item. It also does what the `bootstrap` skill does
// for a checkout that the directory does not have: add_repository, then
// update_repository (COLLIERY-T-0266). bob, a human on the team, raises the ticket
// in the GUI and watches the PR arrive; the forge speaks through signed
// webhooks on a connection alice creates for the repo.
import { expect } from '@playwright/test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { setupTeamRepoAgent, type TeamFixture } from '../fixtures/team';
import { named, runContext } from '../run/context';
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

    await step(alice, 'sets up a team with a repository, a coding agent and a forge webhook (setup)', async () => {
      team = await setupTeamRepoAgent(alice, ledger, 'ios');
      const api = await alice.api();
      // bob is the human on this team: the one who raises the ticket below.
      const members = (await api.get('/api/members?limit=100')).items ?? [];
      const bobRow = members.find((m: any) => m.email === bob.credentials.email);
      await (await alice.cli()).ok(['teams', 'members', 'add', team.teamId, '--user', bobRow.user_id]);
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

    await step.composeOnly(
      agent,
      'finds its repository has no code index yet and asks Kairos to build one (MCP)',
      'the compose server takes a build request and runs no builder (KAIROS_CODE_INDEX_POLL_SECS=0); ' +
        'on a deployment the builder would fetch a repository that does not exist',
      async () => {
        const mcp = await agent.mcp();
        const asked = await mcp.call('rebuild_code_index', { repository: team.repoSlug });
        expect(asked).toContain(`Kairos builds the code index of the repository ${team.repoSlug} again`);
        const run = asked.match(/run ([0-9a-f-]{36})/)?.[1];
        expect(run).toBeTruthy();
        // One run at a time: a second ask while it runs is refused.
        const again = await mcp.refused('rebuild_code_index', { repository: team.repoSlug });
        expect(again).toContain('CODE_INDEX_BUILD_RUNNING');
        return { repository: team.repoSlug, run, second_ask: 'CODE_INDEX_BUILD_RUNNING' };
      },
    );

    await step(agent, 'adds a second repository of its team to the directory, then corrects its description', async () => {
      // COLLIERY-T-0266: the agent stands in a checkout that the directory
      // does not have. Until `add_repository` it had to leave MCP for
      // `kairos repos create`.
      const mcp = await agent.mcp();
      const slug = named('ios-tools');
      const fullName = `acme/${slug}`;
      const absent = await mcp.call('list_repositories', { team: team.teamSlug });
      expect(absent).not.toContain(fullName);
      const added = await mcp.call('add_repository', {
        slug,
        forge: 'github',
        repo_full_name: fullName,
        repo_url: `https://github.com/${fullName}`,
        team: team.teamSlug,
        description: 'Build tools. Run `make test`.',
      });
      const api = await alice.api();
      ledger.add({ kind: 'repository', label: slug, delete: async () => { await api.delete(`/api/repositories/${slug}`); } });
      expect(added).toBe(`Added repository ${slug}: github ${fullName} (owner: ${team.teamSlug}, default branch main).`);
      const updated = await mcp.call('update_repository', {
        repository: slug,
        description: 'Build tools. Run `make test` before each pull request.',
        default_branch: 'trunk',
      });
      expect(updated).toBe(`Updated repository ${slug}: description, default_branch.`);
      const repo = await mcp.call('get_repository', { repository: slug });
      expect(repo).toContain('- default branch: trunk');
      expect(repo).toContain('before each pull request');
      expect(repo).toContain(`- owner team: ${team.teamSlug}`);
      // The agent is not in the team `platform`, so it cannot add a
      // repository for that team. The refusal names the team.
      const refused = await mcp.refused('add_repository', {
        forge: 'github',
        repo_full_name: `acme/${named('not-mine')}`,
        repo_url: `https://github.com/acme/${named('not-mine')}`,
        team: 'platform',
      });
      expect(refused).toContain('FORBIDDEN');
      expect(refused).toContain('the team "platform"');
      return { added: slug, changed: 'description, default_branch', refused_for: 'platform' };
    });

    await step(bob, 'raises a task on the team board, binds it to the repository and moves it to Todo', async () => {
      const page = await bob.gui();
      await openBoard(page, team.boardSlug);
      const title = named('task: offline receipts cache');
      await page.getByRole('button', { name: 'New task', exact: true }).click();
      const modal = page.locator('.cl-modal');
      await modal.locator('input.cl-input').first().fill(title);
      // The New task modal offers every repository of the tenant, the
      // team's own first (COLLIERY-T-0221).
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

    await step(agent, 'writes where the work is: the branch over MCP, then the commit from the CLI', async () => {
      // The live state of the task (KAIROS-A-0029), as the implement skill
      // writes it: one patch over MCP, one from the terminal.
      const branch = code.toLowerCase();
      const mcp = await agent.mcp();
      const set = await mcp.call('set_state', { short_code: code, branch, next_step: 'Write the cache, then open the PR.' });
      expect(set).toContain(`State of ${code}:`);
      expect(set).toContain(branch);
      const cli = await agent.cli();
      await cli.ok(['state', code, '--commit', '1a2b3c4', '--pushed']);
      const item = await mcp.call('get_item', { short_code: code });
      expect(item, 'the branch of the MCP write is kept').toContain(branch);
      expect(item, 'the commit of the CLI write is there').toContain('1a2b3c4');
      return { task: code, branch, commit: '1a2b3c4', pushed: true };
    });

    await step(agent, 'binds its checkout to the team board, then talks to Kairos through the CLI proxy, as the plugin does', async () => {
      const ctx = runContext();
      const checkout = fs.mkdtempSync(path.join(os.tmpdir(), `kairos-uat-${ctx.run}-checkout-`));
      const cli = await agent.cli();
      await cli.ok([
        'config', 'project', checkout, '--path',
        '--deployment', ctx.server, '--tenant', ctx.tenant,
        '--repository', team.repoSlug, '--team-board', team.boardSlug,
      ]);
      const shown = JSON.stringify(await cli.json(['config', 'show', checkout]));
      expect(shown, 'the folder names the board').toContain(team.boardSlug);
      expect(shown, 'and the repository').toContain(team.repoSlug);

      // The plugin starts `kairos mcp-proxy` and speaks MCP on its stdio.
      const requests = [
        { jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-03-26', capabilities: {}, clientInfo: { name: 'uat', version: '0' } } },
        { jsonrpc: '2.0', method: 'notifications/initialized' },
        { jsonrpc: '2.0', id: 2, method: 'tools/call', params: { name: 'whoami', arguments: {} } },
      ].map((r) => JSON.stringify(r)).join('\n') + '\n';
      const proxied = await cli.runWithInput(['mcp-proxy'], requests, (out) => out.includes('"id":2'));
      expect(proxied.stdout, `the proxy answered whoami: ${proxied.stderr}`).toContain('"id":2');
      expect(proxied.stdout).toContain(team.teamSlug);
      fs.rmSync(checkout, { recursive: true, force: true });
      return { checkout: 'bound to ' + team.boardSlug, proxy: 'whoami answered' };
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

    await step(agent, 'reopens the task when the change has to come back: it goes to Backlog (MCP)', async () => {
      const mcp = await agent.mcp();
      const reopened = await mcp.call('reopen_item', { short_code: code });
      expect(reopened).toContain(code);
      const cli = await agent.cli();
      const task = await cli.json(['tasks', 'get', code]);
      const board = await (await alice.api()).get(`/api/boards/${team.boardId}`);
      const columnName = board.columns.find((c: any) => c.id === task.column_id)?.name;
      expect(columnName).toBe('Backlog');
      return { task: code, reopened_to: columnName };
    });
  },
);
