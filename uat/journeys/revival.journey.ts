// J20 — "Work that was put away comes back" (KAIROS-I-0015,
// KAIROS-A-0020). The other end of `housekeeping`: that journey proves work
// can be put away and stops obstructing the guards. This one proves the
// decision that followed — archiving is a visibility default and nothing
// more, so the work is still there to be found, read, and if need be
// brought back.
//
// The story is the one an organisation actually lives: a quarter closes,
// tickets are put away, and months later somebody asks "didn't we already
// look at this?". Under the old behaviour that question had no answer — the
// item and its history both 404'd (KAIROS-T-0151). Now it does.
//
// It also covers the refusal, which is the part most likely to rot: a
// restore whose home is gone must say WHAT is gone rather than silently
// re-homing the work, because the column a card was put away in is the
// placement its record is evidence of.
import { expect } from '@playwright/test';
import { teamFixture } from '../fixtures/team';
import { named } from '../run/context';
import { journey, step } from '../run/narrate';
import { card, openBoard } from '../surfaces/gui';
import { shortCodes } from '../surfaces/mcp';

journey(
  'revival',
  'Work that was put away comes back',
  { humans: ['alice'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    // Its own team; suffix distinct from mobile/ios/infra/ops/crm/data/legacy/srch.
    const team = teamFixture(alice, ledger, 'revive');
    // A word no other run will match, so the search hit stays on page 1
    // however often this deployment has been driven (the e2e spec learned
    // this the hard way — see KAIROS-T-0163).
    const topic = `zarquon${Math.random().toString(36).slice(2, 8)}`;
    let shelved = '';
    let orphan = '';

    await step(alice, 'has a team whose last quarter is finished and put away', async () => {
      const observed = await team.createTeam();
      const cli = await alice.cli();
      const api = await alice.api();

      const made = await cli.json([
        'tasks', 'create', '--board', team.fixture.boardId!,
        '--title', named(`task: ${topic} rate limiting`),
        '--content', `We looked at ${topic} rate limiting and shelved it.`,
      ]);
      shelved = made.short_code;
      ledger.add({ kind: 'task', label: shelved, delete: async () => { await api.delete(`/api/tasks/${shelved}`); } });

      // Give it a second version, so there is a history worth reading back.
      await cli.ok(['tasks', 'edit', shelved, '--content', `Decision: defer ${topic} until we have numbers.`]);
      await cli.ok(['tasks', 'delete', shelved, '--confirm']);

      const page = await alice.gui();
      await openBoard(page, team.fixture.boardSlug!);
      await expect(card(page, shelved)).toHaveCount(0);
      return { team: observed.team, put_away: shelved, topic };
    });

    await step(alice, 'months later, asks whether they ever looked at this — and finds it', async () => {
      const cli = await alice.cli();
      // Without the flag the work is invisible, which is the whole point of
      // putting it away; with it, the same text query reaches it. Before
      // KAIROS-T-0157 the flag was a silent no-op next to `--query`: the
      // candidate sets were intersected and archived ids were dropped
      // before it was ever consulted, so this step is the regression test
      // for a bug that returned "no matches" rather than an error.
      const hidden = await cli.json(['search', '--query', topic, '--limit', '50']);
      const hiddenCodes = [
        ...(hidden.results?.tasks ?? []),
        ...(hidden.results?.documents ?? []),
      ].map((r: any) => r.short_code);
      expect(hiddenCodes, 'put-away work stays out of an ordinary search').not.toContain(shelved);

      const found = await cli.json(['search', '--query', topic, '--include-deleted', '--limit', '50']);
      const foundRows = [...(found.results?.tasks ?? [])];
      const hit = foundRows.find((r: any) => r.short_code === shelved);
      expect(hit, 'asking for put-away work finds it by the same words').toBeTruthy();
      expect(hit.archived_at, 'and the hit says it is put away').toBeTruthy();
      return { query: topic, without_flag: hiddenCodes.length, with_flag: shelved, marked: true };
    });

    await step(alice, 'reads what it actually said, which is the answer she came for', async () => {
      const mcp = await alice.mcp();
      const shown = await mcp.call('get_item', { short_code: shelved });
      expect(shown).toContain('ARCHIVED');
      expect(shown, 'the content survived being put away').toContain(topic);

      // The audit answer proper: copy-forward history (KAIROS-A-0004) exists
      // to reconstruct what a record said at a point in time, and it used to
      // go dark exactly when that mattered.
      const history = await mcp.call('get_history', { short_code: shelved });
      expect(history).toContain('v2');
      expect(history).toContain('v1');
      const v1 = await mcp.call('get_history', { short_code: shelved, version: 1 });
      expect(v1, 'the first version still reads').toContain('shelved');
      return { read: shelved, versions_available: 2, first_version_still_reads: true };
    });

    await step(alice, 'decides the work is live again and puts it back', async () => {
      const mcp = await alice.mcp();
      const restored = await mcp.call('restore_item', { short_code: shelved });
      expect(restored).toContain(shelved);

      const page = await alice.gui();
      await openBoard(page, team.fixture.boardSlug!);
      await expect(card(page, shelved), 'the card is on the board again').toBeVisible();

      const api = await alice.api();
      const item = await api.get(`/api/tasks/${shelved}`);
      expect(item.archived_at, 'and it is not marked put-away any more').toBeFalsy();

      // …and it is editable again, which is the point of restoring rather
      // than merely reading: put-away work is frozen by construction.
      const cli = await alice.cli();
      await cli.ok(['tasks', 'edit', shelved, '--content', `Picking ${topic} back up.`]);
      return { restored: shelved, back_on_board: true, editable_again: true };
    });

    await step(alice, 'finds that work whose home is gone is refused, and told why', async () => {
      const api = await alice.api();
      const cli = await alice.cli();

      // Park a card in a column of its own, put it away, then retire the
      // column — the shape KAIROS-T-0161 made possible and KAIROS-T-0160
      // has to cope with.
      const board = await api.get(`/api/boards/${team.fixture.boardId}`);
      const parked = await api.post('/api/boards/' + team.fixture.boardId + '/columns', {
        name: named('Shelf').slice(0, 40),
        position: board.columns.length,
      });
      const made = await cli.json([
        'tasks', 'create', '--board', team.fixture.boardId!,
        '--column', parked.id,
        '--title', named('task: parked on a shelf that gets removed'),
      ]);
      orphan = made.short_code;
      ledger.add({ kind: 'task', label: orphan, delete: async () => { await api.delete(`/api/tasks/${orphan}`); } });

      await cli.ok(['tasks', 'delete', orphan, '--confirm']);
      const removed = await api.raw('DELETE', `/api/boards/${team.fixture.boardId}/columns/${parked.id}`);
      expect(removed.status, 'a column whose only card is put away can be retired').toBeLessThan(300);

      const refused = await api.raw('POST', `/api/tasks/${orphan}/restore`);
      expect(refused.status).toBe(422);
      expect(refused.body?.error?.code).toBe('RESTORE_BLOCKED');
      const missing = (refused.body?.error?.details?.missing ?? []) as string[];
      expect(missing.join(' '), 'the refusal names what is gone').toMatch(/column/i);

      // The refusal is not a dead end: the record is still readable, which
      // is what lets someone decide where to put it instead.
      const still = await api.get(`/api/tasks/${orphan}`);
      expect(still.archived_at).toBeTruthy();
      return {
        refused: orphan,
        code: 'RESTORE_BLOCKED',
        missing: missing.join('; '),
        still_readable: true,
      };
    });

    await step(alice, 'can still see the whole quarter when she asks for it', async () => {
      const api = await alice.api();
      // The closing claim, and the one that makes this initiative worth the
      // work: nothing has to be guessed at. The put-away card is in the
      // listing when asked for, marked, and absent when not.
      const wide = await api.get(`/api/boards/${team.fixture.boardId}/items?include_deleted=true`);
      const wideCodes = (wide.columns ?? []).flatMap((c: any) => [
        ...(c.tasks ?? []), ...(c.strategies ?? []), ...(c.initiatives ?? []), ...(c.adrs ?? []),
      ]).map((t: any) => t.short_code);
      expect(wideCodes, 'the put-away card is there when asked for').toContain(orphan);

      const narrow = await api.get(`/api/boards/${team.fixture.boardId}/items`);
      const narrowCodes = (narrow.columns ?? []).flatMap((c: any) => c.tasks ?? []).map((t: any) => t.short_code);
      expect(narrowCodes, 'and out of the way when not').not.toContain(orphan);
      expect(narrowCodes, 'while the restored card is simply live').toContain(shelved);
      return {
        board: team.fixture.boardSlug,
        with_put_away: wideCodes.length,
        default_view: narrowCodes.length,
        restored_is_live: shelved,
      };
    });
  },
);
