// J12 — "An operator checks the deployment, then deletes something big"
// (KAIROS-I-0014). The only journey whose persona is not doing product
// work: alice is on the other side of the deployment, holding a runbook.
//
// Two halves, and they are the same instinct. First: look before you
// trust — liveness, readiness, the scrape endpoint and what the
// deployment believes about its own identity provider, all read the way a
// probe or a scraper reads them, with no credentials at all. Then: look
// before you destroy — a cancelled workstream has to go, and the cascade
// preview exists precisely because the delete confirm can only warn about
// DIRECT children while a soft delete takes the whole subtree
// (KAIROS-T-0051). A preview that disagrees with the delete that follows
// is worse than no preview, so the journey holds them to being identical.
import { expect } from '@playwright/test';
import { named, runContext } from '../run/context';
import { journey, step } from '../run/narrate';

/** One Prometheus sample value, by metric name and label text. */
function sample(text: string, needle: string): number | undefined {
  const line = text.split('\n').find((l) => l.startsWith(needle));
  if (!line) return undefined;
  return Number(line.slice(line.lastIndexOf(' ') + 1));
}

journey(
  'operations',
  'An operator checks the deployment, then deletes something big',
  { humans: ['alice'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    const ctx = runContext();
    // An orchestrator probe and a Prometheus scraper hold no token; these
    // requests carry none, which is half of what is being tested.
    const probe = (path: string) => fetch(ctx.server + path);

    await step(alice, 'checks the deployment is alive and ready, the way the orchestrator does — with no credentials', async () => {
      const health = await probe('/healthz');
      expect(health.status, '/healthz answers an unauthenticated probe').toBe(200);
      const ready = await probe('/readyz');
      expect(ready.status, '/readyz answers an unauthenticated probe').toBe(200);
      const readyBody = (await ready.text()).trim();
      // Readiness is database connectivity AND no pending migrations
      // (KAIROS-A-0013) — a binary ahead of its schema must not take
      // traffic, so "ready" is a claim, not a ping.
      expect(readyBody).toBe('ready');
      // The probes sit outside the auth stack ON PURPOSE, and nothing else
      // does: the same request to the API without a token is refused.
      const guarded = await probe('/api/whoami');
      expect(guarded.status, 'the API itself still demands a token').toBe(401);
      return { healthz: health.status, readyz: `${ready.status} ${readyBody}`, api_without_token: guarded.status };
    });

    await step(alice, 'scrapes /metrics and watches her own request land in the tenant counter', async () => {
      const first = await probe('/metrics');
      expect(first.status).toBe(200);
      expect(first.headers.get('content-type') ?? '').toContain('text/plain');
      const before = await first.text();
      for (const family of [
        'http_request_duration_seconds',
        'http_requests_by_tenant_total',
        'kairos_db_pool_connections',
      ]) {
        expect(before, `${family} is exposed`).toContain(`# TYPE ${family}`);
      }
      const label = `http_requests_by_tenant_total{tenant="${ctx.tenant}"}`;
      const was = sample(before, label) ?? 0;
      // A scrape endpoint that renders but never counts is worse than
      // none: it reads healthy forever. So make a real request and insist
      // the number moves.
      const api = await alice.api();
      await api.whoami();
      const after = await (await probe('/metrics')).text();
      const now = sample(after, label);
      expect(now, `${label} is present after a request`).toBeGreaterThan(was);
      const pool = sample(after, 'kairos_db_pool_connections{pool="async",state="total"}');
      expect(pool, 'the async pool reports its size live').toBeGreaterThan(0);
      return {
        families: 3,
        tenant_counter: `${was} → ${now}`,
        async_pool_connections: pool,
      };
    });

    await step(alice, 'reads /api/config to see what the deployment believes about its own identity provider', async () => {
      const res = await probe('/api/config');
      expect(res.status, 'the login bootstrap is readable before anyone has logged in').toBe(200);
      const config = await res.json();
      // The classic broken deployment: the app advertises one issuer and
      // people log in against another. Her personas authenticated against
      // UAT_ISSUER minutes ago; the deployment must name the same one.
      expect(config.issuer).toBe(ctx.issuer);
      expect(config.client_id).toBeTruthy();
      expect(config.authorization_endpoint).toContain(config.issuer);
      return {
        issuer: config.issuer,
        client_id: config.client_id,
        api_bearer: config.api_bearer,
        matches_the_idp_people_log_into: true,
      };
    });

    let strategy = '';
    let initiative = '';
    const tasks: string[] = [];
    await step(alice, 'stages the cancelled workstream she has been asked to remove: a strategy, its initiative, three tasks (CLI + API)', async () => {
      const cli = await alice.cli();
      const api = await alice.api();
      // Ledgered before the story deletes them, and tolerant of a 404 in
      // teardown: the whole point of the journey is that they are gone.
      const forget = (family: string, code: string) => async () => {
        const res = await api.raw('DELETE', `/api/${family}/${code}`);
        if (res.status !== 404 && (res.status < 200 || res.status >= 300)) {
          throw new Error(`DELETE /api/${family}/${code} -> ${res.status}`);
        }
      };
      const strategyBoard = await api.boardBySlug('strategy');
      const initiativeBoard = await api.boardBySlug('initiatives');
      const deliveryBoard = await api.boardBySlug(`${process.env.UAT_TEAM ?? 'platform'}-delivery`);

      const s = await cli.json(['strategies', 'create', '--board', strategyBoard.id, '--title', named('strategy: retire the legacy portal')]);
      strategy = s.short_code;
      ledger.add({ kind: 'strategy', label: strategy, delete: forget('strategies', strategy) });
      const i = await cli.json(['initiatives', 'create', '--board', initiativeBoard.id, '--title', named('initiative: legacy portal shutdown')]);
      initiative = i.short_code;
      ledger.add({ kind: 'initiative', label: initiative, delete: forget('initiatives', initiative) });
      for (const what of ['drain traffic', 'archive the database', 'decommission the hosts']) {
        const t = await cli.json(['tasks', 'create', '--board', deliveryBoard.id, '--title', named(`task: ${what}`)]);
        tasks.push(t.short_code);
        ledger.add({ kind: 'task', label: t.short_code, delete: forget('tasks', t.short_code) });
      }
      // There is no CLI noun for relationships; an operator wiring a tree
      // up scripts the API (KAIROS-A-0006: org admin only).
      await api.post('/api/relationships', {
        source_short_code: strategy, target_short_code: initiative, relationship: 'parent',
      });
      for (const code of tasks) {
        await api.post('/api/relationships', {
          source_short_code: initiative, target_short_code: code, relationship: 'parent',
        });
      }
      return { strategy, initiative, tasks: tasks.join(', ') };
    });

    let preview: string[] = [];
    await step(alice, 'previews the cascade before touching anything — and it is four items deep, not one', async () => {
      const api = await alice.api();
      const response = await api.get(`/api/strategies/${strategy}/cascade-preview`);
      preview = [...response.cascaded_short_codes].sort();
      expect(response.short_code).toBe(strategy);
      expect(response.cascade_count).toBe(preview.length);
      expect(preview).toEqual([initiative, ...tasks].sort());
      // The reason this endpoint exists (KAIROS-T-0051): a delete confirm
      // that lists DIRECT children would have warned about one item while
      // the delete took four.
      const edges = await api.get(`/api/strategies/${strategy}/relationships`);
      const direct = (edges.outgoing ?? []).find((g: any) => g.relationship === 'parent')?.items ?? [];
      expect(direct.length).toBe(1);
      expect(preview.length).toBeGreaterThan(direct.length);
      // A preview is a read: the tree is untouched, so she can still
      // change her mind.
      for (const code of tasks) expect((await api.raw('GET', `/api/tasks/${code}`)).status).toBe(200);
      return {
        root: strategy,
        direct_children: direct.length,
        cascade_count: response.cascade_count,
        would_remove: preview.join(', '),
        tree_still_live: true,
      };
    });

    await step(alice, 'deletes the strategy, and the damage is exactly what the preview promised', async () => {
      const api = await alice.api();
      const deleted = await api.delete(`/api/strategies/${strategy}`);
      const actual = [...deleted.cascaded_short_codes].sort();
      // THE assertion of this journey. Preview and delete share one BFS in
      // kairos-core; if they ever stop agreeing, every confirm dialog in
      // the product has been lying.
      expect(actual).toEqual(preview);
      expect(deleted.cascade_count).toBe(preview.length);
      return { deleted: strategy, cascade_count: deleted.cascade_count, matched_preview: true };
    });

    await step(alice, 'confirms the children went with it, and that nothing is unrecoverable', async () => {
      const api = await alice.api();
      const gone: number[] = [];
      for (const code of [initiative, ...tasks]) {
        const family = code === initiative ? 'initiatives' : 'tasks';
        gone.push((await api.raw('GET', `/api/${family}/${code}`)).status);
      }
      expect(new Set(gone)).toEqual(new Set([404]));
      const board = await api.boardBySlug(`${process.env.UAT_TEAM ?? 'platform'}-delivery`);
      const contents = await api.get(`/api/boards/${board.id}/items`);
      const live = (contents.columns ?? []).flatMap((c: any) => (c.tasks ?? []).map((t: any) => t.short_code));
      for (const code of tasks) expect(live).not.toContain(code);
      // Soft delete, not destruction: an operator who removes the wrong
      // thing needs to know the rows are still there (KAIROS-A-0001).
      // NOTE: `--include-deleted` cannot be combined with `--query` — the
      // full-text view `searchable_items` excludes soft-deleted rows by
      // construction — so the recovery question is asked with filters.
      const cli = await alice.cli();
      const search = (extra: string[]) =>
        cli.json(['search', '--type', 'task', '--board', board.id, '--limit', '100', ...extra]);
      const visible = ((await search([])).results?.tasks ?? []).map((t: any) => t.short_code);
      const withDeleted = ((await search(['--include-deleted'])).results?.tasks ?? []).map((t: any) => t.short_code);
      for (const code of tasks) {
        expect(visible, `${code} is gone from the ordinary view`).not.toContain(code);
        expect(withDeleted, `${code} is still there to recover`).toContain(code);
      }
      return {
        children_now: '404',
        still_on_board: 0,
        board_tasks_visible: visible.length,
        including_deleted: withDeleted.length,
      };
    });

    await step(alice, 'checks the audit trail names her, the item, and every descendant that went with it', async () => {
      const api = await alice.api();
      const me = await api.whoami();
      const feed = await api.get('/api/activity?limit=50');
      const rows = (feed.items ?? feed) as any[];
      const record = rows.find(
        (r) => r.action === 'delete' && (r.details ?? '').includes(`short_code:${strategy}`),
      );
      expect(record, 'the delete is on the record').toBeTruthy();
      // "Who deleted the portal workstream?" is a data question, so read
      // the actor off the row rather than off a rendered label.
      expect(record.actor_id).toBe(me.user.id);
      expect(record.entity_type).toBe('strategy');
      // And the record carries the blast radius, not just the root.
      for (const code of preview) expect(record.details).toContain(code);
      const page = await alice.gui();
      await page.goto('/activity');
      await expect(page.getByText(strategy).first()).toBeVisible({ timeout: 15_000 });
      return {
        action: record.action,
        entity: record.entity_type,
        actor_is_alice: true,
        details: String(record.details).slice(0, 120),
      };
    });
  },
);
