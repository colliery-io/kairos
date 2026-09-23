// J14 — "Someone joins, someone leaves" (KAIROS-I-0014). The first thing
// that happens to an organisation once Kairos is load bearing: the roster
// changes, and the question is whether the work survives it.
//
// Two ways in exist and the journey walks both: team membership, which
// implies the delivery capabilities (A-0006 / KAIROS-T-0072), and an
// explicit board grant for someone who works here without belonging here.
// Then bob leaves, and the point of the last act is that his card is NOT
// his — it belongs to the board, so an offboarding cannot orphan it, but
// it CAN leave a board nobody is left to move. carol, granted minutes
// earlier, is the reason that does not happen.
//
// The org-admin guard closes it: the tenant refuses to be left with no
// admin, whichever way alice asks.
import { expect } from '@playwright/test';
import { teamFixture } from '../fixtures/team';
import { named } from '../run/context';
import { journey, step } from '../run/narrate';
import { card, cardIn, dragCard, openBoard, panel } from '../surfaces/gui';
import { shortCodes } from '../surfaces/mcp';

journey(
  'growing-team',
  'Someone joins, someone leaves',
  { humans: ['alice', 'bob', 'carol'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    const bob = cast.human('bob');
    // The joiner is a real fourth human when the deployment has one; the
    // seed has three, and carol is the one who belongs to no team here.
    const joiner = cast.hasHuman('newhire') ? cast.human('newhire') : cast.human('carol');
    if (joiner.name !== 'newhire') await joiner.token();
    const team = teamFixture(alice, ledger, 'crm');
    let aliceId = '';
    let bobId = '';
    let joinerId = '';
    let work = '';

    // Both cards this journey raises are retired inside the story (a team
    // is only deletable once its board holds no live cards), so teardown
    // tolerates a card that is already gone.
    async function ledgerTask(code: string): Promise<void> {
      const api = await alice.api();
      ledger.add({
        kind: 'task',
        label: code,
        delete: async () => {
          const res = await api.raw('DELETE', `/api/tasks/${code}`);
          if (res.status !== 404 && (res.status < 200 || res.status >= 300)) {
            throw new Error(`DELETE task -> ${res.status}`);
          }
        },
      });
    }

    await step(alice, 'reads the roster before anything changes: who is here, and how many admins', async () => {
      const cli = await alice.cli();
      const members = await cli.json(['members', 'list', '--limit', '100']);
      const rows = (members.items ?? members) as any[];
      const admins = rows.filter((m) => m.role === 'admin');
      aliceId = rows.find((m) => m.email === alice.credentials.email).user_id;
      bobId = rows.find((m) => m.email === bob.credentials.email).user_id;
      joinerId = rows.find((m) => m.email === joiner.credentials.email).user_id;
      expect(admins.map((a: any) => a.email)).toContain(alice.credentials.email);
      return { members: rows.length, admins: admins.length, joiner_played_by: joiner.name };
    });

    await step(alice, 'tries to add a new starter who has never logged in, and is told the contract', async () => {
      const cli = await alice.cli();
      const email = `${named('newstarter')}@kairos.test`;
      const result = await cli.run(['members', 'add', '--email', email]);
      expect(result.code).not.toBe(0);
      // Identities are JIT-provisioned at first login, so "add the person"
      // is always second: the product says so instead of inventing a row.
      const said = `${result.stderr}${result.stdout}`;
      expect(said).toContain('log in once');
      return { email, exit_code: result.code, told: said.replace(/\s+/g, ' ').trim().slice(0, 140) };
    });

    await step(alice, 'stands up the team the group will work on and puts bob on it', async () => {
      const observed = await team.createTeam();
      await team.addMember(bobId);
      const cli = await bob.cli();
      const me = await cli.json(['whoami']);
      expect((me.teams as any[]).map((t) => t.slug)).toContain(team.fixture.teamSlug);
      // Membership alone carries the delivery capabilities — bob holds no
      // grant of his own, which is what makes the removal later bite.
      expect(me.capabilities ?? []).toHaveLength(0);
      return { ...observed, member: bob.credentials.email, explicit_grants: 0 };
    });

    await step(bob, 'does a piece of work on it: a card of his own, taken to Active', async () => {
      const page = await bob.gui();
      await openBoard(page, team.fixture.boardSlug!);
      const title = named('task: migrate the customer import');
      await page.getByRole('button', { name: 'New task', exact: true }).click();
      const modal = page.locator('.cl-modal');
      await modal.locator('input.cl-input').first().fill(title);
      await modal.getByRole('button', { name: 'Create' }).click();
      await expect(modal).toBeHidden();
      work = (await card(page, title).locator('a.kairos-card__code').innerText()).trim();
      await ledgerTask(work);
      await dragCard(page, work, 'Todo');
      await dragCard(page, work, 'Active');
      return { short_code: work, column: 'Active' };
    });

    await step(joiner, 'joins the work but not the team, and the board is shut to her', async () => {
      const mcp = await joiner.mcp();
      const refused = await mcp.refused('create_item', {
        item_type: 'task',
        title: named('task: import validation report'),
        board: team.fixture.boardSlug,
      });
      expect(refused).toContain('manage_tasks');
      // Reads stay open tenant-wide (A-0006) — she can see the work she is
      // about to help with, she just cannot touch it.
      const items = await mcp.call('board_items', { board: team.fixture.boardSlug });
      expect(shortCodes(items)).toContain(work);
      return { refused: refused.split('\n')[0].slice(0, 140), can_read_board: true };
    });

    await step(alice, 'grants her the two capabilities she needs on that board, and nothing else', async () => {
      const page = await alice.gui();
      await page.goto(`/admin/boards/${team.fixture.boardId}`);
      const members = panel(page, 'Members and capabilities');
      await members.locator('.cl-field', { hasText: 'Organization member' }).locator('select')
        .selectOption(joiner.credentials.email);
      // The grant editor is toggles over the A-0006 vocabulary, each row
      // printing the raw capability next to its switch (KAIROS-T-0043).
      for (const capability of ['manage_tasks', 'transition_items']) {
        await members.locator('.cl-group', { has: page.getByText(capability, { exact: true }) })
          .locator('.cl-switch').first().click();
      }
      await members.getByRole('button', { name: 'Add member with grants' }).click();
      await expect(page.getByText(`${joiner.credentials.email} added with`)).toBeVisible({ timeout: 10_000 });
      const api = await alice.api();
      ledger.add({
        kind: 'board-grant',
        label: `${joiner.credentials.email} on ${team.fixture.boardSlug}`,
        delete: async () => { await api.delete(`/api/boards/${team.fixture.boardId}/members/${joinerId}`); },
      });
      const granted = await api.get(`/api/boards/${team.fixture.boardId}/members`);
      const hers = (granted as any[]).find((m: any) => m.email === joiner.credentials.email);
      expect(hers.capabilities.sort()).toEqual(['manage_tasks', 'transition_items']);
      return { member: joiner.credentials.email, grants: hers.capabilities, on_board: team.fixture.boardSlug };
    });

    await step(joiner, 'reads her new powers from the terminal, then uses them', async () => {
      const cli = await joiner.cli();
      const me = await cli.json(['whoami']);
      const board = (me.capabilities as any[]).find((c) => c.board_slug === team.fixture.boardSlug);
      expect(board, 'the grant appears in whoami').toBeTruthy();
      expect(board.grants).toContain('manage_tasks');
      // She is still on no team — the grant is the whole of her access.
      expect((me.teams as any[]).map((t) => t.slug)).not.toContain(team.fixture.teamSlug);
      const mcp = await joiner.mcp();
      const created = await mcp.call('create_item', {
        item_type: 'task',
        title: named('task: import validation report'),
        board: team.fixture.boardSlug,
      });
      const [hers] = shortCodes(created);
      await ledgerTask(hers);
      await mcp.call('delete_item', { short_code: hers, confirm: true });
      return { teams: (me.teams as any[]).length, grants: board.grants, created_then_retired: hers };
    });

    await step(alice, 'offboards bob from the team; his card stays where it is', async () => {
      const cli = await alice.cli();
      await cli.ok(['teams', 'members', 'remove', team.fixture.teamId!, '--user', bobId]);
      const listed = await cli.json(['teams', 'members', 'list', team.fixture.teamId!]);
      const emails = ((listed.items ?? listed) as any[]).map((m: any) => m.email);
      expect(emails).not.toContain(bob.credentials.email);
      // The work is not his to take with him: it belongs to the board.
      const api = await alice.api();
      const task = await api.task(work);
      expect(task.deleted_at ?? null).toBeNull();
      return { removed: bob.credentials.email, team_members_left: emails.length, his_card: work };
    });

    await step(bob, 'finds he can no longer move the card he raised — the powers were the membership', async () => {
      const mcp = await bob.mcp();
      const refused = await mcp.refused('transition_item', { short_code: work, to_column: 'Completed' });
      // He keeps the tenant-wide `file_backlog` every member holds, so the
      // refusal he gets is the cross-team filer's — it names the Backlog
      // whatever column the card is really in. What matters is the
      // capability it asks for, which is the one membership used to imply.
      expect(refused).toContain('transition_items');
      return { required: 'transition_items', refused: refused.replace(/\s+/g, ' ').slice(0, 150), card_still_live: work };
    });

    await step(joiner, 'picks up his in-flight work and finishes it, so the offboarding orphans nothing', async () => {
      const page = await joiner.gui();
      await openBoard(page, team.fixture.boardSlug!);
      await expect(cardIn(page, 'Active', work)).toBeVisible();
      await dragCard(page, work, 'Completed');
      return { inherited: work, column: 'Completed', by: joiner.credentials.email };
    });

    await step(alice, 'is refused when she would leave the organisation with no admin at all', async () => {
      const cli = await alice.cli();
      const demote = await cli.run(['members', 'set-role', aliceId, '--role', 'member']);
      expect(demote.code).not.toBe(0);
      expect(`${demote.stderr}${demote.stdout}`).toContain('LAST_ADMIN');
      const remove = await cli.run(['members', 'remove', aliceId, '--confirm']);
      expect(remove.code).not.toBe(0);
      expect(`${remove.stderr}${remove.stdout}`).toContain('LAST_ADMIN');
      // Both refusals are guards, not accidents: the roster is unchanged.
      const members = await cli.json(['members', 'list', '--limit', '100']);
      const still = ((members.items ?? members) as any[]).find((m: any) => m.email === alice.credentials.email);
      expect(still.role).toBe('admin');
      return { demote_refused: 'LAST_ADMIN', remove_refused: 'LAST_ADMIN', still_admin: still.email };
    });

    await step(joiner, 'retires the finished card, which clears the board for the team to be wound down', async () => {
      const mcp = await joiner.mcp();
      await mcp.call('delete_item', { short_code: work, confirm: true });
      const api = await alice.api();
      const board = await api.get(`/api/boards/${team.fixture.boardId}/items`);
      const remaining = (board.columns ?? []).flatMap((c: any) => c.tasks ?? []);
      expect(remaining.map((t: any) => t.short_code)).not.toContain(work);
      return { deleted: work, live_cards_left: remaining.length };
    });
  },
);
