// The "a team with a repository and a coding agent" fixture: what J1
// (onboarding) builds step by step and J3 (agent-loop) needs to exist.
// Both use this code so the shape is identical in compose and --server
// mode; J1 narrates each part as its own acceptance step, J3 runs it as
// setup. Everything is ledgered in dependency order.
//
// J1 creates a NEW team. J3 cannot: a team whose delivery board has ever
// held an item can never be deleted (soft-deleted rows still pin the
// board — T-0010), so a self-cleaning journey that raises work must use an
// EXISTING team (`onTeam`) and register only its repository and agent
// there, both of which delete cleanly once the run's task is gone.
import type { Persona } from '../personas';
import { named } from '../run/context';
import type { Ledger } from '../run/ledger';
import type { Observed } from '../run/narrate';

export interface TeamFixture {
  teamId: string;
  teamSlug: string;
  boardId: string;
  boardSlug: string;
  columns: string[];
  repoSlug: string;
  repoFullName: string;
  serviceAccountId: string;
  apiKey: string;
}

export interface FixtureSteps {
  /** alice (CLI) creates the team; observe the scaffolded delivery board. */
  createTeam(): Promise<Observed>;
  /** Use an existing team (by slug) instead of creating one. */
  useTeam(slug: string): Promise<Observed>;
  /** alice (CLI) adds a user (by user id) to the team. */
  addMember(userId: string): Promise<void>;
  /** alice (CLI) registers the repository under the team. */
  registerRepository(): Promise<Observed>;
  /** alice (CLI) creates the service account, adds it to the team, mints a key. */
  createAgent(): Promise<Observed>;
  readonly fixture: Partial<TeamFixture>;
  done(): TeamFixture;
}

/**
 * The fixture as separately callable parts so a journey can narrate each
 * one. `suffix` distinguishes multiple fixtures in one run.
 */
export function teamFixture(alice: Persona, ledger: Ledger, suffix = 'mobile'): FixtureSteps {
  const fixture: Partial<TeamFixture> = {};
  const repoSlug = named(`${suffix}-app`);
  const repoFullName = `acme/${repoSlug}`;

  async function observeBoard(teamSlug: string): Promise<Observed> {
    const api = await alice.api();
    const board = await api.boardBySlug(`${teamSlug}-delivery`);
    fixture.boardId = board.id;
    fixture.boardSlug = board.slug;
    fixture.columns = board.columns.map((c: any) => c.name);
    return { team: teamSlug, board: board.slug, columns: fixture.columns, transitions: board.transitions.length };
  }

  return {
    fixture,

    async createTeam() {
      const cli = await alice.cli();
      const api = await alice.api();
      const teamSlug = named(suffix);
      const team = await cli.json([
        'teams', 'create', '--name', `UAT ${suffix} (${teamSlug})`, '--slug', teamSlug, '--type', 'stream_aligned',
      ]);
      fixture.teamId = team.id;
      fixture.teamSlug = team.slug;
      ledger.add({
        kind: 'team',
        label: teamSlug,
        delete: async () => { await api.delete(`/api/teams/${team.id}`); },
      });
      return observeBoard(teamSlug);
    },

    async useTeam(slug: string) {
      const api = await alice.api();
      const team = await api.teamBySlug(slug);
      fixture.teamId = team.id;
      fixture.teamSlug = team.slug;
      return observeBoard(team.slug);
    },

    async addMember(userId: string) {
      const cli = await alice.cli();
      await cli.ok(['teams', 'members', 'add', fixture.teamId!, '--user', userId]);
    },

    async registerRepository() {
      const cli = await alice.cli();
      const api = await alice.api();
      const repo = await cli.json([
        'repos', 'create',
        '--forge', 'github',
        '--name', repoFullName,
        '--repo-url', `https://github.com/${repoFullName}`,
        '--team', fixture.teamSlug!,
        '--slug', repoSlug,
        '--description', 'Flutter app. Run `flutter test` before opening a PR; PRs need one review.',
      ]);
      fixture.repoSlug = repo.slug;
      fixture.repoFullName = repoFullName;
      ledger.add({
        kind: 'repository',
        label: repoSlug,
        delete: async () => { await api.delete(`/api/repositories/${repoSlug}`); },
      });
      return { repository: repo.slug, owner: repo.team?.slug ?? repo.team_id, forge: repo.forge };
    },

    async createAgent() {
      const cli = await alice.cli();
      const api = await alice.api();
      const sa = await cli.json(['service-accounts', 'create', '--name', named(`${suffix}-agent`)]);
      fixture.serviceAccountId = sa.id;
      ledger.add({
        kind: 'service-account',
        label: sa.name,
        delete: async () => { await api.delete(`/api/service-accounts/${sa.id}`); },
      });
      // Team membership is what gives the agent its board powers (A-0006
      // team-implied capabilities) and its repositories in whoami.
      await cli.ok(['teams', 'members', 'add', fixture.teamId!, '--user', sa.id]);
      ledger.add({
        kind: 'team-membership',
        label: `${sa.name} on ${fixture.teamSlug}`,
        delete: async () => { await api.delete(`/api/teams/${fixture.teamId}/members/${sa.id}`); },
      });
      const key = await cli.json([
        'keys', 'create', '--service-account', sa.id, '--name', 'uat-run',
      ]);
      fixture.apiKey = key.key;
      ledger.add({
        kind: 'api-key',
        label: key.prefix ?? key.id,
        delete: async () => { await api.delete(`/api/service-accounts/${sa.id}/keys/${key.id}`); },
      });
      return { service_account: sa.name, key_prefix: key.prefix, team: fixture.teamSlug };
    },

    done() {
      for (const k of ['teamId', 'boardId', 'repoSlug', 'apiKey'] as const) {
        if (!fixture[k]) throw new Error(`team fixture incomplete: ${k} missing`);
      }
      return fixture as TeamFixture;
    },
  };
}

/**
 * A fresh team with its repository and coding agent, as one setup call
 * (J3). Since KAIROS-I-0012 the team is deletable again once its board
 * holds no live cards, so a journey that raises work on it can still
 * clean up: the ledger deletes the task before the team.
 */
export async function setupTeamRepoAgent(alice: Persona, ledger: Ledger, suffix = 'mobile'): Promise<TeamFixture> {
  const steps = teamFixture(alice, ledger, suffix);
  await steps.createTeam();
  await steps.registerRepository();
  await steps.createAgent();
  return steps.done();
}

/**
 * The repository + agent on an EXISTING team (UAT_TEAM, default
 * `platform`) — kept for a deployment where the caller may not create
 * teams.
 */
export async function setupRepoAgentOnTeam(alice: Persona, ledger: Ledger, suffix = 'mobile'): Promise<TeamFixture> {
  const steps = teamFixture(alice, ledger, suffix);
  await steps.useTeam(process.env.UAT_TEAM ?? 'platform');
  await steps.registerRepository();
  await steps.createAgent();
  return steps.done();
}
