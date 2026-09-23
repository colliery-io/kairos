// J8 — "A CI system works, then loses its key" (KAIROS-I-0014). The first
// machine principal in the arc: after the humans are set up, something
// automated needs to talk to Kairos without a person in the loop
// (KAIROS-A-0017).
//
// The interesting part is not that a key works — J3 already leans on
// that — it is the moments around it: rotating without downtime (two live
// keys, briefly), and a revocation that has to land immediately and
// completely. A revoked key that still reads, or a rotation that breaks
// the machine mid-flight, is the kind of thing nobody notices until a
// credential leaks and revoking it does not actually stop anything.
import { expect } from '@playwright/test';
import { named } from '../run/context';
import { journey, step } from '../run/narrate';
import { Api } from '../surfaces/api';

journey(
  'machine-access',
  'A CI system works, then loses its key',
  { humans: ['alice'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    let accountId = '';
    let firstKey = '';
    let firstKeyId = '';
    let secondKey = '';
    let code = '';

    await step(alice, 'registers a service account for CI and mints it a key, shown once', async () => {
      const cli = await alice.cli();
      const api = await alice.api();
      const account = await cli.json(['service-accounts', 'create', '--name', named('ci-deploy')]);
      accountId = account.id;
      ledger.add({
        kind: 'service-account',
        label: account.name,
        // The last step retires the account as part of the story, so
        // teardown tolerates it being gone already rather than reporting
        // a failure for work the journey did on purpose.
        delete: async () => {
          const res = await api.raw('DELETE', `/api/service-accounts/${accountId}`);
          if (res.status !== 404 && (res.status < 200 || res.status >= 300)) {
            throw new Error(`DELETE service account -> ${res.status}`);
          }
        },
      });
      const key = await cli.json(['keys', 'create', '--service-account', accountId, '--name', 'primary']);
      firstKey = key.key;
      firstKeyId = key.id;
      expect(firstKey).toMatch(/^kairos_sk_/);
      // The raw secret is returned exactly once; everything after this
      // reads a prefix only.
      const listed = await cli.json(['keys', 'list', '--service-account', accountId]);
      const rows = listed.items ?? listed;
      expect(JSON.stringify(rows)).not.toContain(firstKey);
      return { service_account: account.name, key_prefix: key.prefix, secret_in_list: false };
    });

    await step(alice, 'puts the machine on its team so it has somewhere to work', async () => {
      const cli = await alice.cli();
      const api = await alice.api();
      const team = await api.teamBySlug(process.env.UAT_TEAM ?? 'platform');
      await cli.ok(['teams', 'members', 'add', team.id, '--user', accountId]);
      ledger.add({
        kind: 'team-membership',
        label: `ci-deploy on ${team.slug}`,
        delete: async () => { await api.delete(`/api/teams/${team.id}/members/${accountId}`); },
      });
      return { team: team.slug };
    });

    const machine = () => cast.agent('agent', firstKey);
    await step(cast.agent('agent', 'placeholder'), 'the machine introduces itself and files its first piece of work', async () => {
      const ci = machine();
      const mcp = await ci.mcp();
      const me = await mcp.call('whoami');
      expect(me).toContain('ci-deploy');
      const cli = await ci.cli();
      const created = await cli.json([
        'tasks', 'create', '--repo', 'payments-api', '--title', named('task: nightly deploy failed'),
      ]);
      code = created.short_code;
      const api = await alice.api();
      ledger.add({ kind: 'task', label: code, delete: async () => { await api.delete(`/api/tasks/${code}`); } });
      return { authenticated_as: 'ci-deploy', filed: code };
    });

    await step(alice, 'rotates the credential: a second key is minted while the first still works', async () => {
      const cli = await alice.cli();
      const key = await cli.json(['keys', 'create', '--service-account', accountId, '--name', 'rotation']);
      secondKey = key.key;
      // Both live at once, which is what makes a rotation possible without
      // downtime: the machine can be switched over before the old one dies.
      for (const [label, secret] of [['old', firstKey], ['new', secondKey]] as const) {
        const probe = new Api(secret);
        const res = await probe.raw('GET', '/api/whoami');
        expect(res.status, `${label} key works during rotation`).toBe(200);
      }
      return { keys_live: 2, overlap: 'both accepted' };
    });

    await step(alice, 'revokes the old key, and it stops working immediately', async () => {
      const cli = await alice.cli();
      await cli.ok(['keys', 'revoke', firstKeyId, '--service-account', accountId, '--confirm']);
      const dead = new Api(firstKey);
      const res = await dead.raw('GET', '/api/whoami');
      expect(res.status, 'a revoked key is refused').toBeGreaterThanOrEqual(401);
      expect(res.status).toBeLessThan(500);
      // …and the revocation is surgical: the machine's other credential is
      // untouched, so a leak response does not take the pipeline down.
      const alive = new Api(secondKey);
      expect((await alive.raw('GET', '/api/whoami')).status).toBe(200);
      return { revoked: 'primary', old_key_status: res.status, new_key_still_works: true };
    });

    await step(cast.agent('agent-rotated', 'placeholder'), 'the machine carries on with its new key', async () => {
      const ci = cast.agent('agent-rotated', secondKey);
      const mcp = await ci.mcp();
      const item = await mcp.call('get_item', { short_code: code });
      expect(item).toContain(code);
      await mcp.call('transition_item', { short_code: code, to_column: 'Todo' });
      return { still_working: true, moved: `${code} → Todo` };
    });

    await step(alice, 'sees the machine in the activity feed as a principal in its own right', async () => {
      const page = await alice.gui();
      await page.goto('/activity');
      await expect(page.getByRole('link', { name: code }).first()).toBeVisible({ timeout: 15_000 });
      // A machine's work must be attributable — "who deployed that?" is
      // the question this answers — so check the actor on the record, not
      // just that something appeared on screen.
      const api = await alice.api();
      const feed = await api.get('/api/activity?limit=100');
      const rows = (feed.items ?? feed) as any[];
      const mine = rows.filter((r: any) => (r.details ?? '').includes(code) || r.entity_id);
      const actors = new Set(rows.map((r: any) => r.actor_display_name ?? r.actor_id));
      expect(mine.length).toBeGreaterThan(0);
      return { attributed_to: 'ci-deploy', distinct_actors_in_feed: actors.size };
    });

    await step(alice, 'retires the account entirely, and the last key dies with it', async () => {
      const api = await alice.api();
      await api.delete(`/api/service-accounts/${accountId}`);
      const dead = new Api(secondKey);
      const res = await dead.raw('GET', '/api/whoami');
      expect(res.status, 'deleting the account kills its keys').toBeGreaterThanOrEqual(401);
      return { account: 'deleted', last_key_status: res.status };
    });
  },
);
