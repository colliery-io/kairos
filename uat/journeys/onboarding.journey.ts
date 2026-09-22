// J1 — "An organisation is set up and a new engineer finds their team"
// (KAIROS-I-0011 D4). alice, the org admin, sets a team up from the CLI:
// team → member → repository → coding agent. bob, the new member, finds it
// all in the GUI; the agent finds it over MCP. Provisioning a tenant is
// compose-only (needs a deployment-admin token and a throwaway tenant).
import { expect } from '@playwright/test';
import { teamFixture } from '../fixtures/team';
import { named } from '../run/context';
import { journey, step } from '../run/narrate';
import { openTeam, panel } from '../surfaces/gui';

journey(
  'onboarding',
  'An organisation is set up and a new engineer finds their team',
  { humans: ['alice', 'bob'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    // The new hire is a real second human when one is configured; the
    // seed has none, so bob plays the part and the report says so.
    const newhire = cast.hasHuman('newhire') ? cast.human('newhire') : cast.human('bob');
    if (newhire.name !== 'newhire') await newhire.token();
    const team = teamFixture(alice, ledger, 'mobile');

    await step.composeOnly(alice, 'provisions a new tenant as deployment admin', 'needs a deployment-admin token and a throwaway tenant', async () => {
      const api = await alice.api();
      const slug = named('tenant').replace(/[^a-z0-9_-]/g, '-');
      const created = await api.post('/api/admin/tenants', { slug, name: `UAT tenant ${slug}` });
      ledger.add({
        kind: 'tenant',
        label: slug,
        delete: async () => { await api.delete(`/api/admin/tenants/${slug}?confirm=true`); },
      });
      const listed = await api.get('/api/admin/tenants');
      const slugs = (Array.isArray(listed) ? listed : listed.items ?? []).map((t: any) => t.slug);
      expect(slugs).toContain(slug);
      return { tenant: slug, initial_admin: created.initial_admin?.email ?? created.initial_admin };
    });

    await step(alice, 'creates a stream-aligned team from the CLI and finds its delivery board scaffolded', async () => {
      const observed = await team.createTeam();
      expect(observed.columns as string[]).toContain('Backlog');
      expect(observed.transitions as number).toBeGreaterThan(0);
      return observed;
    });

    await step(alice, `adds ${newhire.name} to the team`, async () => {
      const cli = await alice.cli();
      const members = await cli.json(['members', 'list', '--limit', '100']);
      const user = (members.items ?? members).find((m: any) => m.email === newhire.credentials.email);
      expect(user, `${newhire.credentials.email} is an org member`).toBeTruthy();
      await team.addMember(user.user_id);
      return { member: user.email, played_by: newhire.name === 'newhire' ? 'newhire' : 'bob (no newhire persona configured)' };
    });

    await step(newhire, 'sees the team and its board in `kairos whoami`', async () => {
      const cli = await newhire.cli();
      const me = await cli.json(['whoami']);
      const mine = (me.teams as any[]).map((t) => t.slug);
      expect(mine).toContain(team.fixture.teamSlug);
      return { teams: mine, implicit: me.implicit };
    });

    await step(alice, 'registers the team\'s repository with a "how to work here" description', async () => {
      const observed = await team.registerRepository();
      expect(observed.owner).toBe(team.fixture.teamSlug);
      return observed;
    });

    await step(alice, 'creates a coding-agent service account on the team and mints its key (shown once)', async () => {
      const observed = await team.createAgent();
      expect(team.fixture.apiKey).toMatch(/^kairos_sk_/);
      return observed;
    });

    await step(newhire, 'logs into the GUI and finds the team under My teams with its repository', async () => {
      const page = await newhire.gui();
      const nav = page.locator('.kairos-nav__section', { hasText: 'My teams' });
      await expect(nav.getByRole('link', { name: `UAT mobile (${team.fixture.teamSlug})` })).toBeVisible();
      await openTeam(page, team.fixture.teamSlug!);
      const repos = panel(page, 'Repositories');
      await expect(repos.locator(`[data-repo="${team.fixture.repoSlug}"]`)).toBeVisible();
      await expect(repos.locator(`[data-repo="${team.fixture.repoSlug}"]`)).toContainText(team.fixture.repoFullName!);
      await page.goto(`/boards/${team.fixture.boardSlug}`);
      await expect(page.locator('.kairos-board__column-head', { hasText: 'Backlog' }).first()).toBeVisible();
      const cards = await page.locator('article.kairos-card').count();
      return { team_page: `/teams/${team.fixture.teamSlug}`, board: team.fixture.boardSlug, cards_on_board: cards };
    });

    const agent = cast.agent('agent', team.done().apiKey);
    await step(agent, 'introduces itself over MCP and sees its team, its repository and file_backlog', async () => {
      const mcp = await agent.mcp();
      const text = await mcp.call('whoami');
      expect(text).toContain(`- ${team.fixture.teamSlug} —`);
      expect(text).toContain(`- ${team.fixture.repoSlug} — ${team.fixture.repoFullName} (owner: ${team.fixture.teamSlug})`);
      expect(text).toContain('file_backlog');
      const repo = await mcp.call('get_repository', { repository: team.fixture.repoSlug });
      expect(repo).toContain('flutter test');
      return {
        implicit: text.match(/- implicit[^:]*: ([a-z_, ]+)/)?.[1]?.trim(),
        description_seen: 'yes',
      };
    });

    await step(alice, 'is refused when deleting the team while it still owns the repository', async () => {
      const api = await alice.api();
      const res = await api.raw('DELETE', `/api/teams/${team.fixture.teamId}`);
      expect(res.status).toBe(409);
      return { status: res.status, message: res.body?.error?.message ?? res.body?.message ?? JSON.stringify(res.body).slice(0, 120) };
    });
    // Teardown (ledger, reverse order): key → service account → repository → team → tenant.
  },
);
