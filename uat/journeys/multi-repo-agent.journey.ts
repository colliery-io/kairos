// J22 — "A coding agent works across three repositories and keeps them
// straight" (KAIROS-I-0017). J3 (`agent-loop`) is one agent, one repository,
// one ticket. This is the shape agents are actually deployed in: the same
// agent moves between codebases all day, and the interesting failures are all
// about crossing between them — carrying one repo's conventions into another,
// filing work where it was standing rather than where it belongs, and
// disturbing a board it only meant to read.
//
// It uses the three seeded repositories rather than creating any, because they
// already span two owner teams and carry genuinely different instructions
// (`cargo test`, Terraform plan output, `trunk build`). The only thing this
// journey creates is one service account that is a member of BOTH teams —
// which is the whole premise: what an agent may do in a repository is decided
// by its team membership, not by what it happens to be working on.
//
// It also covers relevance ranking at the MCP surface (KAIROS-T-0186). Before
// that task a text search came back in creation order, so "search before you
// file" — the one habit that stops an agent re-solving solved work — returned
// the best answer wherever it happened to land.
import { expect } from '@playwright/test';
import { named } from '../run/context';
import { journey, step } from '../run/narrate';
import { field, shortCodes } from '../surfaces/mcp';

/** The seeded estate: two teams, three repositories, three sets of habits. */
const PLATFORM_REPO = process.env.UAT_PLATFORM_REPO ?? 'payments-api';
const INFRA_REPO = process.env.UAT_INFRA_REPO ?? 'platform-infra';
const WEB_REPO = process.env.UAT_WEB_REPO ?? 'portal-web';
const ALL_REPOS = [PLATFORM_REPO, INFRA_REPO, WEB_REPO];

/** Short codes a repository currently holds, sorted so two reads compare. */
async function repoContents(mcp: { call(t: string, a?: unknown): Promise<string> }, repo: string): Promise<string[]> {
  const text = await mcp.call('search', { filter: { repository: repo }, limit: 100 });
  return shortCodes(text).sort();
}

journey(
  'multi-repo-agent',
  'A coding agent works across three repositories and keeps them straight',
  { humans: ['alice'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    let apiKey = '';
    let platformBoard = '';
    let webBoard = '';
    let filedCrossRepo = '';
    let webTask = '';
    let rankedFirst = '';
    let rankedSecond = '';

    await step(alice, 'creates one agent and puts it on both teams, so it spans all three repositories (setup)', async () => {
      const cli = await alice.cli();
      const api = await alice.api();
      const sa = await cli.json(['service-accounts', 'create', '--name', named('fleet-agent')]);
      ledger.add({
        kind: 'service-account',
        label: sa.name,
        delete: async () => { await api.delete(`/api/service-accounts/${sa.id}`); },
      });
      // Membership is the grant (A-0006 team-implied capabilities). Two
      // memberships is what makes this agent multi-repo rather than a second
      // single-repo agent.
      const teams: string[] = [];
      for (const repo of [PLATFORM_REPO, WEB_REPO]) {
        const owner = (await api.get(`/api/repositories/${repo}`)).team?.slug
          ?? (await api.get(`/api/repositories/${repo}`)).team_id;
        const team = await api.teamBySlug(owner);
        if (teams.includes(team.slug)) continue;
        teams.push(team.slug);
        await cli.ok(['teams', 'members', 'add', team.id, '--user', sa.id]);
        ledger.add({
          kind: 'team-membership',
          label: `${sa.name} on ${team.slug}`,
          delete: async () => { await api.delete(`/api/teams/${team.id}/members/${sa.id}`); },
        });
      }
      const key = await cli.json(['keys', 'create', '--service-account', sa.id, '--name', 'uat-run']);
      apiKey = key.key;
      ledger.add({
        kind: 'api-key',
        label: key.prefix ?? key.id,
        delete: async () => { await api.delete(`/api/service-accounts/${sa.id}/keys/${key.id}`); },
      });
      return { service_account: sa.name, teams, key_prefix: key.prefix };
    });

    const agent = cast.agent('agent', apiKey!);

    await step(agent, 'bootstraps over MCP and discovers it answers to two teams and three codebases', async () => {
      const mcp = await agent.mcp();
      const me = await mcp.call('whoami');
      const directory = await mcp.call('list_repositories');
      for (const repo of ALL_REPOS) {
        expect(directory, `${repo} is in the agent's directory`).toContain(repo);
      }
      // Two teams, not one: the estate is the union of its memberships. Read
      // the `## Teams` section specifically — whoami lists repositories in the
      // same `- slug — name` shape one section further down.
      const teamSection = me.split('## Teams')[1]?.split('\n##')[0] ?? '';
      const teams = [...new Set([...teamSection.matchAll(/^- ([a-z0-9-]+) —/gm)].map((m) => m[1]))];
      expect(teams.length, 'the agent belongs to both owning teams').toBe(2);
      // And its own repository list agrees with the directory.
      const mine = me.split("## My teams' repositories")[1]?.split('\n##')[0] ?? '';
      for (const repo of ALL_REPOS) {
        expect(mine, `${repo} is one of the agent's own`).toContain(repo);
      }
      return { teams, repositories: ALL_REPOS };
    });

    await step(agent, 'reads each repository and gets three different sets of instructions, not one', async () => {
      const mcp = await agent.mcp();
      const habits: Record<string, string> = {};
      const owners: Record<string, string> = {};
      for (const repo of ALL_REPOS) {
        const text = await mcp.call('get_repository', { repository: repo });
        owners[repo] = field(text, '- owner team') ?? '';
        habits[repo] = text.split('## How to work here')[1]?.split('##')[0]?.trim().split('\n')[0] ?? '';
        expect(habits[repo], `${repo} says how to work in it`).toBeTruthy();
        if (repo === PLATFORM_REPO) platformBoard = field(text, '- delivery board')?.split(' ')[0] ?? '';
        if (repo === WEB_REPO) webBoard = field(text, '- delivery board')?.split(' ')[0] ?? '';
      }
      // The claim worth asserting: the instructions are per-repository. An
      // agent that reads one and assumes the rest runs the wrong test command
      // in two of three codebases, and nothing tells it off.
      const distinct = new Set(Object.values(habits));
      expect(distinct.size, 'each repository has its own instructions').toBe(ALL_REPOS.length);
      expect(platformBoard).toBeTruthy();
      expect(webBoard).toBeTruthy();
      expect(platformBoard, 'the two repos it files across sit on different boards').not.toBe(webBoard);
      return {
        boards: `${PLATFORM_REPO} → ${platformBoard}, ${WEB_REPO} → ${webBoard}`,
        owners: ALL_REPOS.map((r) => `${r} → ${owners[r]}`),
        instructions: ALL_REPOS.map((r) => `${r}: ${habits[r].slice(0, 52)}`),
      };
    });

    await step(agent, `raises two overlapping tickets on ${WEB_REPO}, one naming the problem in its title and one only in its body`, async () => {
      const mcp = await agent.mcp();
      const api = await alice.api();
      // Deliberate shape for the ranking step below: the same two words, once
      // in a title and once in prose.
      const first = await mcp.call('create_item', {
        item_type: 'task',
        title: named('ledger reconciliation mismatch'),
        repository: WEB_REPO,
        content: 'The finance page disagrees with the statement after a refund.',
      });
      [rankedFirst] = shortCodes(first);
      ledger.add({ kind: 'task', label: rankedFirst, delete: async () => { await api.delete(`/api/tasks/${rankedFirst}`); } });
      const second = await mcp.call('create_item', {
        item_type: 'task',
        title: named('finance page totals'),
        repository: WEB_REPO,
        content: 'A ledger reconciliation problem shows up here when a refund lands mid-month.',
      });
      [rankedSecond] = shortCodes(second);
      ledger.add({ kind: 'task', label: rankedSecond, delete: async () => { await api.delete(`/api/tasks/${rankedSecond}`); } });
      expect(rankedFirst).toBeTruthy();
      expect(rankedSecond).toBeTruthy();
      return { in_title: rankedFirst, in_body: rankedSecond, repository: WEB_REPO };
    });

    await step(agent, 'searches before filing anything else, and the best match comes back first without being asked', async () => {
      const mcp = await agent.mcp();
      // No `sort`: ranking is the default when there is a query (T-0186).
      const text = await mcp.call('search', {
        q: 'ledger reconciliation',
        filter: { repository: WEB_REPO },
        limit: 25,
      });
      const order = shortCodes(text);
      expect(order, 'both overlapping tickets are found').toContain(rankedFirst);
      expect(order, 'both overlapping tickets are found').toContain(rankedSecond);
      expect(order.indexOf(rankedFirst), 'the title match outranks the body match')
        .toBeLessThan(order.indexOf(rankedSecond));
      // And chronology is still available, disagreeing — so the assertion
      // above cannot be passing by accident of creation order.
      const byAge = shortCodes(await mcp.call('search', {
        q: 'ledger reconciliation',
        filter: { repository: WEB_REPO },
        sort: { field: 'created_at', order: 'desc' },
        limit: 25,
      }));
      expect(byAge.indexOf(rankedSecond), 'newest-first puts the other one on top')
        .toBeLessThan(byAge.indexOf(rankedFirst));
      return { ranked: order.slice(0, 2), newest_first: byAge.slice(0, 2), sort_asked_for: 'none' };
    });

    await step(agent, 'is refused when it asks for relevance with nothing to be relevant to', async () => {
      const mcp = await agent.mcp();
      const refusal = await mcp.refused('search', {
        filter: { repository: WEB_REPO },
        sort: { field: 'relevance', order: 'desc' },
      });
      expect(refusal).toContain('relevance');
      // A silent fall back to date order would have handed the agent a
      // chronological list it believed was ranked.
      return { refused: refusal.split('\n')[0].slice(0, 140) };
    });

    await step(agent, `files the defect against ${PLATFORM_REPO} even though it was working in ${WEB_REPO}, and it lands on platform's board`, async () => {
      const mcp = await agent.mcp();
      const api = await alice.api();
      const text = await mcp.call('create_item', {
        item_type: 'task',
        title: named('payments: refund rounds the wrong way'),
        repository: PLATFORM_REPO,
        content: `Found while working in ${WEB_REPO}: the refund total is rounded before tax, not after. Done = the service rounds after tax and the finance page agrees.`,
      });
      [filedCrossRepo] = shortCodes(text);
      ledger.add({ kind: 'task', label: filedCrossRepo, delete: async () => { await api.delete(`/api/tasks/${filedCrossRepo}`); } });
      const item = await mcp.call('get_item', { short_code: filedCrossRepo });
      const board = field(item, '- board') ?? '';
      // The routing claim in create_item's own description: the repository
      // decides the board, not the agent's current working context.
      expect(board, 'it routed to the repository owner, not where the agent stood').toContain(platformBoard);
      expect(board).not.toContain(webBoard);
      expect(board).toContain('Backlog');
      return { short_code: filedCrossRepo, filed_from: WEB_REPO, landed_on: board };
    });

    await step(agent, 'draws the cross-repository dependency it can see and the humans cannot', async () => {
      const mcp = await agent.mcp();
      const api = await alice.api();
      const text = await mcp.call('create_item', {
        item_type: 'task',
        title: named('portal: show the corrected refund total'),
        repository: WEB_REPO,
      });
      [webTask] = shortCodes(text);
      ledger.add({ kind: 'task', label: webTask, delete: async () => { await api.delete(`/api/tasks/${webTask}`); } });
      const linked = await mcp.call('link_items', { source: filedCrossRepo, target: webTask, relationship: 'blocks' });
      const blocked = await mcp.call('get_item', { short_code: webTask });
      expect(blocked).toContain(filedCrossRepo);
      return { edge: `${filedCrossRepo} blocks ${webTask}`, across: `${PLATFORM_REPO} → ${WEB_REPO}`, tool_said: linked.split('\n')[0] };
    });

    await step(agent, 'starts work in one repository and leaves the other two exactly as it found them', async () => {
      const mcp = await agent.mcp();
      const before: Record<string, string[]> = {};
      for (const repo of ALL_REPOS) before[repo] = await repoContents(mcp, repo);

      // Membership, not authorship, is what lets it move a card on platform's
      // board — it filed this into their Backlog, and it is also one of them.
      const moved = await mcp.call('transition_item', { short_code: filedCrossRepo, to_column: 'Todo' });
      expect(moved).toContain(filedCrossRepo);
      const item = await mcp.call('get_item', { short_code: filedCrossRepo });
      // get_item prints board and column on one line: `- board: x / column: y`.
      expect(field(item, '- board')).toContain('column: Todo');

      const after: Record<string, string[]> = {};
      for (const repo of ALL_REPOS) after[repo] = await repoContents(mcp, repo);
      for (const repo of [INFRA_REPO, WEB_REPO]) {
        expect(after[repo], `${repo} is untouched`).toEqual(before[repo]);
      }
      return {
        worked: `${PLATFORM_REPO}/${filedCrossRepo} → Todo`,
        untouched: [INFRA_REPO, WEB_REPO].map((r) => `${r}: ${after[r].length} items, unchanged`),
      };
    });

    await step(agent, 'reads its whole estate one repository at a time to decide what to pick up next', async () => {
      const mcp = await agent.mcp();
      const perRepo: Record<string, number> = {};
      for (const repo of ALL_REPOS) {
        const codes = await repoContents(mcp, repo);
        perRepo[repo] = codes.length;
        // A repository-scoped read returns that repository only. Without
        // that, a three-repo agent cannot tell whose work it is looking at.
        for (const other of ALL_REPOS.filter((r) => r !== repo)) {
          const otherCodes = await repoContents(mcp, other);
          expect(codes.filter((c) => otherCodes.includes(c)), `${repo} and ${other} do not bleed`).toEqual([]);
        }
      }
      expect(await repoContents(mcp, PLATFORM_REPO)).toContain(filedCrossRepo);
      expect(await repoContents(mcp, WEB_REPO)).toContain(webTask);
      return {
        open_per_repository: ALL_REPOS.map((r) => `${r}: ${perRepo[r]}`),
        next_up: `${filedCrossRepo} in ${PLATFORM_REPO}`,
      };
    });

    await step(alice, 'reviews what the agent did across both boards, and the edge it drew survives her read', async () => {
      const cli = await alice.cli();
      const platform = await cli.json(['search', '--repo', PLATFORM_REPO, '--limit', '100']);
      const web = await cli.json(['search', '--repo', WEB_REPO, '--limit', '100']);
      const codes = (hits: any) => (hits.results?.tasks ?? []).map((t: any) => t.short_code);
      expect(codes(platform)).toContain(filedCrossRepo);
      expect(codes(web)).toContain(webTask);
      expect(codes(web), 'the cross-filed task is not on the web side').not.toContain(filedCrossRepo);
      const item = await (await alice.api()).task(webTask);
      expect(item).toBeTruthy();
      return {
        per_repository: [`${PLATFORM_REPO}: ${codes(platform).length}`, `${WEB_REPO}: ${codes(web).length}`],
        cross_filed: filedCrossRepo,
        edge_intact: `${filedCrossRepo} blocks ${webTask}`,
      };
    });
  },
);
