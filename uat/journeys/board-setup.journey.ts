// J7 — "An admin shapes a new team's board" (KAIROS-I-0013 D5). The
// `/admin/*` surfaces had no journey: a team could be created by J1 and
// then nobody ever configured anything on it.
//
// The story is the one that makes configuration worth having: alice adds
// a column and the transitions to reach it, defines a field the team will
// actually fill in, wires the team into a delivery stream — and then bob
// drags a card through the transition she invented minutes earlier. If
// configuration did not reach the board, that last step is where it shows.
import { expect } from '@playwright/test';
import { teamFixture } from '../fixtures/team';
import { named } from '../run/context';
import { journey, step } from '../run/narrate';
import { card, cardIn, dragCard, openBoard } from '../surfaces/gui';
import { shortCodes } from '../surfaces/mcp';

journey(
  'board-setup',
  'An admin shapes a new team\'s board',
  { humans: ['alice', 'bob'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    const bob = cast.human('bob');
    // Its own team (suffix distinct from J1's `mobile` and J3's `ios`, or
    // the slugs collide in one run) so the configuration under test is
    // never the seed's.
    const team = teamFixture(alice, ledger, 'infra');
    const fieldSlug = `uat-risk-${Date.now().toString(36)}`;
    let code = '';

    await step(alice, 'creates the team and reads the board it was given', async () => {
      const observed = await team.createTeam();
      const cli = await alice.cli();
      const shown = await cli.json(['boards', 'show', team.fixture.boardId!]);
      expect(shown).toBeTruthy();
      const cli_columns = (observed.columns as string[]) ?? [];
      expect(cli_columns).not.toContain('Review');
      return { ...observed, review_column_exists: false };
    });

    await step(alice, 'adds bob so someone will actually work the board', async () => {
      const cli = await alice.cli();
      const members = await cli.json(['members', 'list', '--limit', '100']);
      const row = (members.items ?? members).find((m: any) => m.email === bob.credentials.email);
      await team.addMember(row.user_id);
      return { member: row.email };
    });

    await step(alice, 'adds a Review column and the two transitions that make it reachable', async () => {
      const page = await alice.gui();
      await page.goto(`/admin/boards/${team.fixture.boardId}`);
      const columns = page.locator('.cl-panel', { hasText: 'Columns' }).first();
      await columns.locator('input[placeholder="New column name"]').fill('Review');
      await columns.getByRole('button', { name: 'Add column' }).click();
      await expect(page.getByText('Review').first()).toBeVisible({ timeout: 10_000 });

      const transitions = page.locator('.cl-panel', { hasText: 'Transitions' }).first();
      // `hasText` is a substring match and several captions mention "To",
      // so select the field by its exact label.
      const field = (label: string) =>
        transitions.locator('.cl-field', {
          has: page.getByText(label, { exact: true }),
        });
      for (const [from, to] of [
        ['Active', 'Review'],
        ['Review', 'Completed'],
      ]) {
        await field('From').locator('select').selectOption(from);
        await field('To').locator('select').selectOption(to);
        await transitions.getByRole('button', { name: 'Add transition' }).click();
        await expect(transitions.getByText(`${from} → ${to}`).first()).toBeVisible({ timeout: 10_000 });
      }
      const api = await alice.api();
      const board = await api.get(`/api/boards/${team.fixture.boardId}`);
      const names = board.columns.map((c: any) => c.name);
      expect(names).toContain('Review');
      return { columns: names, transitions: board.transitions.length };
    });

    await step(alice, 'defines a field the team will fill in, scoped to tasks', async () => {
      const page = await alice.gui();
      await page.goto('/admin/metadata');
      const form = page.locator('.cl-panel', { hasText: 'Create definition' }).first();
      await form.locator('.cl-field', { hasText: 'Name' }).locator('input').fill('UAT Risk');
      await form.locator('.cl-field', { hasText: 'Slug' }).locator('input').fill(fieldSlug);
      await form.getByRole('button', { name: 'Create definition' }).click();
      await expect(page.getByText(fieldSlug).first()).toBeVisible({ timeout: 10_000 });
      const api = await alice.api();
      const defs = (await api.get('/api/metadata-definitions')) as any;
      const mine = (defs.items ?? defs).find((d: any) => d.slug === fieldSlug);
      expect(mine, 'the definition exists').toBeTruthy();
      ledger.add({
        kind: 'metadata-definition',
        label: fieldSlug,
        delete: async () => { await api.delete(`/api/metadata-definitions/${mine.id}`); },
      });
      return { field: fieldSlug, type: mine.field_type ?? mine.type };
    });

    await step(bob, 'raises a card on the new board and stamps the field over MCP', async () => {
      const mcp = await bob.mcp();
      const created = await mcp.call('create_item', {
        item_type: 'task',
        title: named('task: pilot the new workflow'),
        board: team.fixture.boardSlug,
      });
      [code] = shortCodes(created);
      const api = await alice.api();
      ledger.add({ kind: 'task', label: code, delete: async () => { await api.delete(`/api/tasks/${code}`); } });
      await mcp.call('set_metadata', { short_code: code, values: { [fieldSlug]: 'high' } });
      const item = await mcp.call('get_item', { short_code: code });
      expect(item).toContain(fieldSlug);
      expect(item).toContain('high');
      return { short_code: code, stamped: `${fieldSlug}=high` };
    });

    await step(alice, 'finds that card by the field she just invented', async () => {
      const cli = await alice.cli();
      const hits = await cli.json(['search', '--metadata', `${fieldSlug}=high`, '--limit', '50']);
      const codes = (hits.results?.tasks ?? []).map((t: any) => t.short_code);
      expect(codes).toContain(code);
      return { query: `${fieldSlug}=high`, hits: codes.length };
    });

    await step(alice, 'wires the team into a delivery stream', async () => {
      const page = await alice.gui();
      const streamSlug = named('stream').slice(0, 40);
      await page.goto('/admin/streams');
      const form = page.locator('.cl-panel', { hasText: 'Create stream' }).first();
      await form.locator('.cl-field', { hasText: 'Name' }).locator('input').fill(`UAT ${streamSlug}`);
      await form.locator('.cl-field', { hasText: 'Slug' }).locator('input').fill(streamSlug);
      await form.getByRole('button', { name: 'Create stream' }).click();
      await expect(page.getByText(streamSlug).first()).toBeVisible({ timeout: 10_000 });
      const api = await alice.api();
      const streams = (await api.get('/api/delivery-streams')) as any;
      const mine = (streams.items ?? streams).find((s: any) => s.slug === streamSlug);
      expect(mine, 'the stream exists').toBeTruthy();
      ledger.add({
        kind: 'delivery-stream',
        label: streamSlug,
        delete: async () => { await api.delete(`/api/delivery-streams/${mine.id}`); },
      });
      await api.post(`/api/delivery-streams/${mine.id}/teams`, { team_id: team.fixture.teamId });
      const cli = await alice.cli();
      const listed = await cli.json(['streams', 'list', '--limit', '100']);
      const slugs = (listed.items ?? listed).map((s: any) => s.slug);
      expect(slugs).toContain(streamSlug);
      return { stream: streamSlug, team_attached: team.fixture.teamSlug };
    });

    await step(bob, 'drags the card through the transition alice invented minutes ago', async () => {
      const page = await bob.gui();
      await openBoard(page, team.fixture.boardSlug!);
      await expect(card(page, code)).toBeVisible();
      await dragCard(page, code, 'Todo');
      await dragCard(page, code, 'Active');
      // The point of the whole journey: configuration reached the board.
      await dragCard(page, code, 'Review');
      await expect(cardIn(page, 'Review', code)).toBeVisible();
      return { task: code, column: 'Review', via: 'Active → Review (added today)' };
    });

    await step(bob, 'retires the card, which clears the board for the team to be wound down', async () => {
      const mcp = await bob.mcp();
      // The stamp has to come off first, and that is a product fact rather
      // than a tidiness preference: once the card is deleted its metadata
      // row is unreachable (`set_metadata` answers "no live item", and
      // there is no route that clears it) but still counted, so the
      // definition can never be retired again — `?include_deleted=true`
      // and `?force=true` are both refused. See KAIROS-T-0152.
      await mcp.call('set_metadata', { short_code: code, values: { [fieldSlug]: null } });
      await mcp.call('delete_item', { short_code: code, confirm: true });
      const api = await alice.api();
      const board = await api.get(`/api/boards/${team.fixture.boardId}/items`);
      const remaining = (board.columns ?? []).flatMap((c: any) => c.tasks ?? []);
      expect(remaining.map((t: any) => t.short_code)).not.toContain(code);
      return { deleted: code, live_cards_left: remaining.length };
    });
  },
);
