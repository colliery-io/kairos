// J15 — "Two teams become one" (KAIROS-I-0014). The mature-org event
// nothing tests: a team is wound down and its work absorbed.
//
// KAIROS-I-0012 wrote the rule — a team deletes once it owns no
// repositories (409 naming them) and its board holds no live cards (422
// naming them) — and J6 walks it on an empty team. This journey walks it
// under load, which is where the interesting part is: re-homing a
// repository RETARGETS everything bound to it. The card on the old board
// can now only go one place, the machine scoped to that repository can
// still SEE its queue but no longer act on it, and both of those are
// invisible until someone tries. Routing that goes stale after a re-home
// is the catch.
import { expect } from '@playwright/test';
import { teamFixture } from '../fixtures/team';
import { named } from '../run/context';
import { journey, step } from '../run/narrate';
import { cardIn, dragCard, openBoard } from '../surfaces/gui';
import { shortCodes } from '../surfaces/mcp';

// A third board to try to park the card on — the seed's platform board,
// which owns neither team's repositories.
const OTHER_BOARD = `${process.env.UAT_TEAM ?? 'platform'}-delivery`;

journey(
  'reorg',
  'Two teams become one',
  { humans: ['alice', 'bob'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    const bob = cast.human('bob');
    // The surviving team comes from the shared fixture; the one being wound
    // down is built by hand, because the journey DELETES it and every
    // teardown closure for it has to tolerate the thing already being gone.
    const surviving = teamFixture(alice, ledger, 'data');
    let oldTeamId = '';
    let oldTeamSlug = '';
    let oldBoardId = '';
    let oldBoardSlug = '';
    let repoSlug = '';
    let agentKey = '';
    let agentId = '';
    let bobId = '';
    let work = '';
    let survivingBoardId = '';

    await step(alice, 'has two teams to merge: the surviving one, and the one being wound down with its service and its agent (setup)', async () => {
      const cli = await alice.cli();
      const api = await alice.api();
      const survivor = await surviving.createTeam();
      survivingBoardId = surviving.fixture.boardId!;

      oldTeamSlug = named('srch');
      const old = await cli.json([
        'teams', 'create', '--name', `UAT search (${oldTeamSlug})`, '--slug', oldTeamSlug, '--type', 'stream_aligned',
      ]);
      oldTeamId = old.id;
      ledger.add({
        kind: 'team',
        label: oldTeamSlug,
        // Wound down inside the story; teardown tolerates its absence.
        delete: async () => {
          const res = await api.raw('DELETE', `/api/teams/${oldTeamId}`);
          if (res.status !== 404 && (res.status < 200 || res.status >= 300)) {
            throw new Error(`DELETE team -> ${res.status}: ${JSON.stringify(res.body)}`);
          }
        },
      });
      const oldBoard = await api.boardBySlug(`${oldTeamSlug}-delivery`);
      oldBoardId = oldBoard.id;
      oldBoardSlug = oldBoard.slug;

      repoSlug = named('srch-index');
      await cli.ok([
        'repos', 'create', '--forge', 'github', '--name', `acme/${repoSlug}`,
        '--repo-url', `https://github.com/acme/${repoSlug}`,
        '--team', oldTeamSlug, '--slug', repoSlug,
        '--description', 'The search index service. Run `cargo test` before opening a PR.',
      ]);
      ledger.add({ kind: 'repository', label: repoSlug, delete: async () => { await api.delete(`/api/repositories/${repoSlug}`); } });

      const account = await cli.json(['service-accounts', 'create', '--name', named('srch-agent')]);
      agentId = account.id;
      ledger.add({ kind: 'service-account', label: account.name, delete: async () => { await api.delete(`/api/service-accounts/${agentId}`); } });
      await cli.ok(['teams', 'members', 'add', oldTeamId, '--user', agentId]);
      const key = await cli.json(['keys', 'create', '--service-account', agentId, '--name', 'uat-run']);
      agentKey = key.key;

      const members = await cli.json(['members', 'list', '--limit', '100']);
      bobId = ((members.items ?? members) as any[]).find((m: any) => m.email === bob.credentials.email).user_id;
      await cli.ok(['teams', 'members', 'add', oldTeamId, '--user', bobId]);
      return {
        surviving_team: survivor.team as string,
        winding_down: oldTeamSlug,
        repository: repoSlug,
        agent: account.name,
        on_both: bob.credentials.email,
      };
    });

    await step(bob, 'has live work on the search board, bound to the service his team owns', async () => {
      const mcp = await bob.mcp();
      const created = await mcp.call('create_item', {
        item_type: 'task',
        title: named('task: rebuild the synonym dictionary'),
        repository: repoSlug,
      });
      [work] = shortCodes(created);
      const api = await alice.api();
      ledger.add({ kind: 'task', label: work, delete: async () => { await api.delete(`/api/tasks/${work}`); } });
      await mcp.call('transition_item', { short_code: work, to_column: 'Todo' });
      const item = await mcp.call('get_item', { short_code: work });
      expect(item).toContain(`board: ${oldBoardSlug}`);
      return { short_code: work, board: oldBoardSlug, column: 'Todo', repository: repoSlug };
    });

    const agent = cast.agent('agent', agentKey!);

    await step(agent, 'has it in its repository queue, the way it starts every day', async () => {
      const mcp = await agent.mcp();
      const queue = await mcp.call('board_items', { board: oldBoardSlug, repository: repoSlug });
      expect(shortCodes(queue)).toContain(work);
      return { board: oldBoardSlug, repository: repoSlug, queue: shortCodes(queue).length };
    });

    await step(alice, 'announces the merge and is refused: the team still owns a repository', async () => {
      const cli = await alice.cli();
      const refused = await cli.run(['teams', 'delete', oldTeamId, '--confirm']);
      expect(refused.code).not.toBe(0);
      const said = `${refused.stderr}${refused.stdout}`;
      expect(said).toContain('still owns');
      // The refusal names WHAT is in the way, which is the difference
      // between a guard and an obstacle.
      expect(said).toContain(repoSlug);
      return { refused: said.replace(/\s+/g, ' ').trim().slice(0, 160) };
    });

    await step(alice, 're-homes the service to the surviving team', async () => {
      const cli = await alice.cli();
      await cli.ok(['repos', 'update', repoSlug, '--team', surviving.fixture.teamSlug!]);
      const repo = await cli.json(['repos', 'get', repoSlug]);
      expect(repo.team?.slug ?? repo.team_slug).toBe(surviving.fixture.teamSlug);
      return { repository: repoSlug, owner: surviving.fixture.teamSlug, was: oldTeamSlug };
    });

    await step(alice, 'finds the card\'s routing has gone stale: it may only follow its repository now', async () => {
      const cli = await alice.cli();
      // The card still sits on the OLD board while its repository belongs to
      // the new owner — the exact state this journey exists to catch. Park
      // it anywhere else and the product says where it belongs.
      const refused = await cli.run(['tasks', 'move', work, '--to-board', OTHER_BOARD]);
      expect(refused.code).not.toBe(0);
      const said = `${refused.stderr}${refused.stdout}`;
      expect(said).toContain('REPOSITORY_OWNER_MISMATCH');
      expect(said).toContain(repoSlug);
      expect(said).toContain(survivingBoardId);
      return { still_on: oldBoardSlug, tried: OTHER_BOARD, refused: 'REPOSITORY_OWNER_MISMATCH', must_go_to: surviving.fixture.boardSlug };
    });

    await step(alice, 'is refused again — the board is clear of repositories but not of cards', async () => {
      const cli = await alice.cli();
      const refused = await cli.run(['teams', 'delete', oldTeamId, '--confirm']);
      expect(refused.code).not.toBe(0);
      const said = `${refused.stderr}${refused.stdout}`;
      expect(said).toContain('BOARD_NOT_EMPTY');
      expect(said).toContain(work);
      return { refused: 'BOARD_NOT_EMPTY', named: work };
    });

    await step(alice, 'moves the card across, and it lands in the surviving board\'s entry column', async () => {
      const cli = await alice.cli();
      await cli.ok(['tasks', 'move', work, '--to-board', surviving.fixture.boardSlug!]);
      const api = await alice.api();
      const task = await api.task(work);
      expect(task.board_id).toBe(survivingBoardId);
      const board = await api.get(`/api/boards/${survivingBoardId}`);
      const landed = board.columns.find((c: any) => c.id === task.column_id)?.name;
      expect(landed).toBe('Backlog');
      return { task: work, from: oldBoardSlug, to: surviving.fixture.boardSlug, column: landed };
    });

    await step(agent, 'still finds its queue after the re-home — and cannot touch it', async () => {
      const mcp = await agent.mcp();
      // The catch, both halves: the queue query follows the REPOSITORY, so
      // the work is exactly where the machine would look for it…
      const queue = await mcp.call('board_items', { board: surviving.fixture.boardSlug, repository: repoSlug });
      expect(shortCodes(queue)).toContain(work);
      // …but its powers came from a team that no longer owns the service,
      // so an unattended agent would sit here failing every transition.
      const refused = await mcp.refused('transition_item', { short_code: work, to_column: 'Todo' });
      expect(refused).toContain('transition_items');
      return { queue_found: work, on_board: surviving.fixture.boardSlug, refused: 'transition_items' };
    });

    await step(alice, 'moves the people and the machines across too', async () => {
      const cli = await alice.cli();
      const api = await alice.api();
      for (const [label, userId] of [['bob', bobId]] as const) {
        await cli.ok(['teams', 'members', 'add', surviving.fixture.teamId!, '--user', userId]);
        ledger.add({
          kind: 'team-membership',
          label: `${label} on ${surviving.fixture.teamSlug}`,
          delete: async () => { await api.delete(`/api/teams/${surviving.fixture.teamId}/members/${userId}`); },
        });
      }
      await cli.ok(['teams', 'members', 'add', surviving.fixture.teamId!, '--user', agentId]);
      ledger.add({
        kind: 'team-membership',
        label: `agent on ${surviving.fixture.teamSlug}`,
        delete: async () => { await api.delete(`/api/teams/${surviving.fixture.teamId}/members/${agentId}`); },
      });
      const listed = await cli.json(['teams', 'members', 'list', surviving.fixture.teamId!]);
      return { team: surviving.fixture.teamSlug, members: ((listed.items ?? listed) as any[]).length };
    });

    await step(agent, 'picks the work back up, on a board it had never heard of this morning', async () => {
      const machine = cast.agent('agent', agentKey);
      const mcp = await machine.mcp();
      await mcp.call('transition_item', { short_code: work, to_column: 'Todo' });
      const item = await mcp.call('get_item', { short_code: work });
      expect(item).toContain(`board: ${surviving.fixture.boardSlug}`);
      expect(item).toContain('column: Todo');
      return { task: work, board: surviving.fixture.boardSlug, column: 'Todo' };
    });

    await step(alice, 'winds the old team down at last: it owns nothing and its board is clear', async () => {
      const cli = await alice.cli();
      await cli.ok(['teams', 'delete', oldTeamId, '--confirm']);
      const teams = await cli.json(['teams', 'list', '--limit', '100']);
      const slugs = ((teams.items ?? teams) as any[]).map((t: any) => t.slug);
      expect(slugs).not.toContain(oldTeamSlug);
      expect(slugs).toContain(surviving.fixture.teamSlug);
      // The board goes with the team (KAIROS-I-0012): there is no orphan
      // board left holding a soft-deleted team's history.
      const api = await alice.api();
      const gone = await api.raw('GET', `/api/boards/${oldBoardId}`);
      expect(gone.status).toBe(404);
      return { deleted: oldTeamSlug, board_status: gone.status, teams_left: slugs.length };
    });

    await step(bob, 'finds his work on the surviving team\'s board and carries on', async () => {
      const page = await bob.gui();
      await openBoard(page, surviving.fixture.boardSlug!);
      await expect(cardIn(page, 'Todo', work)).toBeVisible();
      await dragCard(page, work, 'Active');
      return { task: work, board: surviving.fixture.boardSlug, column: 'Active' };
    });
  },
);
