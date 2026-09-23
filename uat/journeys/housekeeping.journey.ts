// J19 — "Old work is put away" (KAIROS-I-0014). The end of the arc: an
// organisation that has been running on Kairos for a while, closing out a
// quarter.
//
// Archiving IS soft delete here — there is no separate Archived state
// (KAIROS-I-0012 §D1, Dylan's rule: "all cards must be archived or
// moved"). That makes this journey the one that proves the two halves of
// that decision actually hold together — and it found that only one half
// holds. A put-away card stops obstructing the guards, as intended. But
// the item and its version history become unreachable (404), so what the
// ticket SAID is gone from every surface; only the activity trail, which
// records that it existed and who touched it, survives. The journey
// asserts that as it is rather than as we assumed. See KAIROS-T-0151.
import { expect } from '@playwright/test';
import { teamFixture } from '../fixtures/team';
import { named } from '../run/context';
import { journey, step } from '../run/narrate';
import { card, openBoard } from '../surfaces/gui';
import { shortCodes } from '../surfaces/mcp';

journey(
  'housekeeping',
  'Old work is put away',
  { humans: ['alice'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    // Its own team, so the quarter being closed is this journey's and not
    // the seed's. Suffix distinct from mobile/ios/infra/ops/crm.
    const team = teamFixture(alice, ledger, 'legacy');
    const shipped: string[] = [];
    let stillOpen = '';

    await step(alice, 'has a team that has been running for a while: a repository and a quarter of work', async () => {
      const observed = await team.createTeam();
      await team.registerRepository();
      const cli = await alice.cli();
      const api = await alice.api();
      for (const title of ['migrate the billing cron', 'retire the v1 export']) {
        const made = await cli.json([
          'tasks', 'create', '--board', team.fixture.boardId!, '--title', named(`task: ${title}`),
        ]);
        shipped.push(made.short_code);
        ledger.add({ kind: 'task', label: made.short_code, delete: async () => { await api.delete(`/api/tasks/${made.short_code}`); } });
      }
      const open = await cli.json([
        'tasks', 'create', '--repo', team.fixture.repoSlug!, '--title', named('task: v2 export, still going'),
      ]);
      stillOpen = open.short_code;
      ledger.add({ kind: 'task', label: stillOpen, delete: async () => { await api.delete(`/api/tasks/${stillOpen}`); } });
      return { team: observed.team, repository: team.fixture.repoSlug, shipped: shipped.length, still_open: stillOpen };
    });

    await step(alice, 'puts the shipped work away, and the board empties', async () => {
      const cli = await alice.cli();
      for (const code of shipped) await cli.ok(['tasks', 'delete', code, '--confirm']);
      const page = await alice.gui();
      await openBoard(page, team.fixture.boardSlug!);
      for (const code of shipped) await expect(card(page, code)).toHaveCount(0);
      await expect(card(page, stillOpen)).toBeVisible();
      return { archived: shipped, still_on_the_board: stillOpen };
    });

    await step(alice, 'finds the archived work gone from the API entirely — only the activity trail remembers it', async () => {
      const mcp = await alice.mcp();
      const queue = await mcp.call('board_items', { board: team.fixture.boardSlug });
      expect(shortCodes(queue)).not.toContain(shipped[0]);

      const api = await alice.api();
      // What archiving actually does today, asserted as it is rather than
      // as KAIROS-I-0012 assumed: the item AND its version history become
      // unreachable (404), not merely hidden from the queue. The rows are
      // still in the database — the retention sweeper prunes on its own
      // schedule — but nothing serves them, so "what did that ticket say?"
      // has no answer once it is put away. See the task's Status Update.
      const item = await api.raw('GET', `/api/tasks/${shipped[0]}`);
      const history = await api.raw('GET', `/api/tasks/${shipped[0]}/history`);
      expect(item.status).toBe(404);
      expect(history.status).toBe(404);

      // The activity log is what survives, and it is thinner: it records
      // THAT the work existed and what happened to it, not what it said.
      const feed = await api.get('/api/activity?limit=200');
      const rows = (feed.items ?? feed) as any[];
      const mentions = rows.filter((r: any) => (r.details ?? '').includes(shipped[0])).length;
      expect(mentions).toBeGreaterThan(0);
      return {
        off_the_queue: true,
        item_after_archiving: item.status,
        history_after_archiving: history.status,
        activity_rows_naming_it: mentions,
      };
    });

    await step(alice, 'cannot retire the repository yet — a live ticket still points at it', async () => {
      const api = await alice.api();
      const res = await api.raw('DELETE', `/api/repositories/${team.fixture.repoSlug}`);
      expect(res.status).toBe(409);
      const message = res.body?.error?.message ?? '';
      // The guard counts the LIVE ticket, not the archived ones: if
      // archiving did not release the guard, nothing could ever be retired.
      expect(message).toMatch(/task|reference|in use/i);
      return { status: res.status, held_by: stillOpen, message: message.slice(0, 130) };
    });

    await step(alice, 'archives the last ticket, and the repository retires cleanly', async () => {
      const cli = await alice.cli();
      await cli.ok(['tasks', 'delete', stillOpen, '--confirm']);
      const api = await alice.api();
      const res = await api.raw('DELETE', `/api/repositories/${team.fixture.repoSlug}`);
      expect(res.status, 'archived tickets do not hold a repository hostage').toBeGreaterThanOrEqual(200);
      expect(res.status).toBeLessThan(300);
      const listed = await cli.json(['repos', 'list']);
      const slugs = ((listed.items ?? listed) as any[]).map((r: any) => r.slug);
      expect(slugs).not.toContain(team.fixture.repoSlug);
      return { archived: stillOpen, repository_retired: team.fixture.repoSlug };
    });

    await step(alice, 'winds the team down — the board is clear because everything on it was put away', async () => {
      const api = await alice.api();
      const res = await api.raw('DELETE', `/api/teams/${team.fixture.teamId}`);
      // KAIROS-I-0012's whole point, at the end of a real quarter rather
      // than in a two-card test: archived cards no longer pin the team.
      expect(res.status, 'a quarter of archived work does not make a team immortal').toBeGreaterThanOrEqual(200);
      expect(res.status).toBeLessThan(300);
      const cli = await alice.cli();
      const teams = await cli.json(['teams', 'list', '--limit', '100']);
      const slugs = ((teams.items ?? teams) as any[]).map((t: any) => t.slug);
      expect(slugs).not.toContain(team.fixture.teamSlug);
      return { team_deleted: team.fixture.teamSlug, cards_archived: shipped.length + 1 };
    });

    await step(alice, 'can still answer THAT the team did the work, though no longer what the work said', async () => {
      const api = await alice.api();
      // The closing claim of the arc, stated as it really is: the
      // organisation changed shape around the work and the trail of it
      // survives — who did what, when, to which short code. The content
      // does not. That is a deliberate thing to know about a system of
      // record, not a detail to discover during an audit.
      const feed = await api.get('/api/activity?limit=200');
      const rows = (feed.items ?? feed) as any[];
      const named_rows = rows.filter((r: any) =>
        [...shipped, stillOpen].some((code) => (r.details ?? '').includes(code)),
      );
      expect(named_rows.length).toBeGreaterThan(0);
      const actors = new Set(named_rows.map((r: any) => r.actor_display_name ?? r.actor_id));
      return {
        team: 'gone',
        trail_of_its_work: `${named_rows.length} activity rows`,
        attributable_to: actors.size,
        content_of_archived_work: 'not retrievable (see Status Update)',
      };
    });
  },
);
