// J5 — "An edit goes wrong and the record puts it right" (KAIROS-I-0013
// D3). KAIROS-A-0004 promises versioned content, conflict detection that
// hands you the current state, and a rollback that copies forward rather
// than erasing. Nothing walked that as a person until this journey.
//
// The shape of the story: alice corrects a task, bob overwrites part of it
// from his own session, alice's next save is refused with the current
// state (not a lost update), she reconciles, then makes a mistake and uses
// the history to undo it — and the audit trail shows the whole episode,
// including the mistake. That last part is the point: an audit trail that
// hides the bad version is not an audit trail.
import { expect } from '@playwright/test';
import { named } from '../run/context';
import { journey, step } from '../run/narrate';
import { openItem, panel } from '../surfaces/gui';

journey(
  'audit-trail',
  'An edit goes wrong and the record puts it right',
  { humans: ['alice', 'bob'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    const bob = cast.human('bob');
    let code = '';
    let goodVersion = 0;

    await step(alice, 'writes a task she will spend the rest of this story correcting', async () => {
      const cli = await alice.cli();
      const api = await alice.api();
      // `tasks create --board` takes a UUID (unlike `tasks move
      // --to-board` and `--repo`, which accept a slug) — resolve it.
      const board = await api.boardBySlug('platform-delivery');
      const created = await cli.json([
        'tasks', 'create', '--board', board.id,
        '--title', named('task: retention sweeper thresholds'),
        '--content', 'Sweep history older than 30 days.',
      ]);
      code = created.short_code;
      ledger.add({ kind: 'task', label: code, delete: async () => { await api.delete(`/api/tasks/${code}`); } });
      return { short_code: code, version: created.version };
    });

    await step(alice, 'corrects the threshold over MCP and the version moves up', async () => {
      const mcp = await alice.mcp();
      const before = await mcp.call('get_item', { short_code: code });
      const version = Number(before.match(/- version: (\d+)/)?.[1]);
      expect(version).toBeGreaterThan(0);
      await mcp.call('update_item', {
        short_code: code,
        content: 'Sweep history older than 90 days.',
        version,
      });
      const after = await mcp.call('get_item', { short_code: code });
      goodVersion = Number(after.match(/- version: (\d+)/)?.[1]);
      expect(goodVersion).toBe(version + 1);
      expect(after).toContain('90 days');
      return { from_version: version, to_version: goodVersion };
    });

    await step(bob, 'edits the same task from his own session while alice is still reading hers', async () => {
      const api = await bob.api();
      const current = await api.task(code);
      await api.patch(`/api/tasks/${code}`, {
        title: current.title,
        content: 'Sweep history older than 90 days. Archive to S3 first.',
        version: current.version,
      });
      const after = await api.task(code);
      return { version: after.version, added: 'Archive to S3 first.' };
    });

    await step(alice, 'saves against the version she loaded and is refused — with the current state to reconcile from', async () => {
      const mcp = await alice.mcp();
      // A lost update would be the real failure here; the refusal is the
      // feature, and it has to carry enough to recover without a re-read.
      const refusal = await mcp.refused('update_item', {
        short_code: code,
        content: 'Sweep history older than 7 days.',
        version: goodVersion,
      });
      expect(refusal).toContain('CONFLICT');
      expect(refusal).toContain('Archive to S3 first.');
      const currentVersion = Number(refusal.match(/current is (\d+)/)?.[1] ?? '0');
      expect(currentVersion).toBeGreaterThan(goodVersion);
      return { refused_at: goodVersion, current_version: currentVersion, carried_current_content: true };
    });

    await step(alice, 'makes a small correction with edit_item instead of a whole-content write', async () => {
      const mcp = await alice.mcp();
      const text = await mcp.call('edit_item', {
        short_code: code,
        search: '90 days',
        replace: '45 days',
      });
      expect(text).toBeTruthy();
      const after = await mcp.call('get_item', { short_code: code });
      expect(after).toContain('45 days');
      expect(after).toContain('Archive to S3 first.');
      goodVersion = Number(after.match(/- version: (\d+)/)?.[1]);
      return { replaced: '90 days → 45 days', version: goodVersion, bobs_sentence_kept: true };
    });

    await step(alice, 'then makes a mistake: she blanks the note bob added', async () => {
      const mcp = await alice.mcp();
      await mcp.call('update_item', {
        short_code: code,
        content: 'Sweep everything nightly.',
        version: goodVersion,
      });
      const after = await mcp.call('get_item', { short_code: code });
      expect(after).not.toContain('Archive to S3 first.');
      return { version: Number(after.match(/- version: (\d+)/)?.[1]), lost: "bob's S3 note" };
    });

    await step(alice, 'opens the history and sees every version, hers and bob\'s', async () => {
      const page = await alice.gui();
      await openItem(page, code);
      await page.getByRole('link', { name: 'History', exact: true }).click();
      await page.waitForURL(/\/activity\/history\//);
      const rows = page.locator('tbody tr');
      await expect(rows.first()).toBeVisible();
      const versions = await rows.count();
      expect(versions).toBeGreaterThanOrEqual(4);
      await expect(rows.locator('.cl-pill', { hasText: 'current' })).toHaveCount(1);
      return { versions_listed: versions };
    });

    await step(alice, 'rolls back to the good version — and the snapshot is copied FORWARD, not restored in place', async () => {
      const page = await alice.gui();
      const row = page.locator('tbody tr', { has: page.locator(`input[aria-label="diff from v${goodVersion}"]`) });
      await row.getByRole('button', { name: 'Roll back', exact: true }).click();
      const modal = page.locator('.cl-modal');
      await modal.getByRole('button', { name: 'Roll back', exact: true }).click();
      const notice = page.getByText(/Rolled back: the v\d+ snapshot/);
      await expect(notice).toBeVisible({ timeout: 15_000 });
      const text = await notice.innerText();
      const newVersion = Number(text.match(/new version v(\d+)/)?.[1] ?? '0');
      // A-0004: history is never rewritten. The bad version stays on the
      // record; the fix is a new version on top of it.
      expect(newVersion).toBeGreaterThan(goodVersion + 1);
      return { rolled_back_to: goodVersion, new_version: newVersion, history_rewritten: false };
    });

    await step(alice, 'confirms from the CLI and reads the whole chain over MCP', async () => {
      const cli = await alice.cli();
      const task = await cli.json(['tasks', 'get', code]);
      expect(task.content).toContain('45 days');
      expect(task.content).toContain('Archive to S3 first.');
      const mcp = await alice.mcp();
      // get_history is the version ledger (who, when, which version), not
      // the snapshots — every version alice and bob wrote is on it,
      // including the mistake she just rolled back past.
      const history = await mcp.call('get_history', { short_code: code });
      const versions = [...history.matchAll(/v(\d+)/g)].map((m) => Number(m[1]));
      expect(Math.max(...versions)).toBeGreaterThanOrEqual(6);
      expect(history).toContain(bob.credentials.email.split('@')[0]);
      return {
        content_now: task.content.slice(0, 60),
        versions_on_the_ledger: Math.max(...versions),
        mistake_still_on_the_record: true,
      };
    });

    await step(bob, 'finds the episode in the activity feed, filtered to the item', async () => {
      const page = await bob.gui();
      await page.goto('/activity');
      const filters = panel(page, 'Filters');
      await filters.locator('.cl-field', { hasText: 'Entity short code' }).locator('input').fill(code);
      await filters.getByRole('button', { name: 'Apply' }).click();
      await expect(page.getByRole('link', { name: code }).first()).toBeVisible({ timeout: 15_000 });
      const rows = await page.locator('tbody tr').count();
      return { filtered_to: code, entries: rows };
    });
  },
);
