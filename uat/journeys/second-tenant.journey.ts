// J18 — "The deployment hosts more than one organisation"
// (KAIROS-I-0014). Compose-only from end to end: every step needs a
// deployment-admin token and a throwaway organisation, so a `--server`
// run skips the lot and says why.
//
// What this journey can honestly prove depends on how the deployment
// resolves tenants, and the UAT target is pinned (`KAIROS_SINGLE_TENANT=demo`,
// the evaluation posture from KAIROS-A-0013). So the header a caller
// sends is not consulted at all — which the journey demonstrates rather
// than assumes, by asking for an organisation that does not exist and
// getting the pinned one back. The seam that IS live here is the one that
// matters most anyway: `/api/admin/tenants` is the only cross-tenant
// surface in the product, and everyone who is not the operator is refused
// it outright — they cannot even enumerate the other organisations.
//
// bob is deliberately made the second organisation's first admin: he then
// belongs to TWO organisations and still only ever reaches one, which is
// the sharpest form of the question a person would ask.
import { expect } from '@playwright/test';
import { named, runContext } from '../run/context';
import { journey, step } from '../run/narrate';
import { Api } from '../surfaces/api';
import { McpSession } from '../surfaces/mcp';

const COMPOSE_ONLY = 'needs a deployment-admin token and a throwaway organisation';

journey(
  'second-tenant',
  'The deployment hosts more than one organisation',
  { humans: ['alice', 'bob'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    const bob = cast.human('bob');
    const ctx = runContext();
    const slug = named('org').replace(/[^a-z0-9_-]/g, '-').slice(0, 63);
    let provisioned = false;

    await step.composeOnly(alice, 'provisions a second organisation and makes bob its first admin', COMPOSE_ONLY, async () => {
      const cli = await alice.cli();
      const api = await alice.api();
      // The initial admin must be a user who has logged in at least once
      // (users are JIT-provisioned), and he is named by OIDC sub.
      const members = await api.get('/api/members?limit=100');
      const row = (members.items ?? members).find((m: any) => m.email === bob.credentials.email);
      expect(row, `${bob.credentials.email} has logged in at least once`).toBeTruthy();
      const created = await cli.json([
        'admin', 'tenants', 'create', '--slug', slug, '--name', `UAT second org (${slug})`,
        '--initial-admin', row.external_id,
      ]);
      provisioned = true;
      ledger.add({
        kind: 'tenant',
        label: slug,
        // The story retires it in the last step, so teardown tolerates it
        // already being gone.
        delete: async () => {
          if (!provisioned) return;
          const res = await api.raw('DELETE', `/api/admin/tenants/${slug}?confirm=true`);
          if (res.status !== 404 && (res.status < 200 || res.status >= 300)) {
            throw new Error(`DELETE tenant ${slug} -> ${res.status}`);
          }
        },
      });
      // A new organisation arrives COMPLETE and shares nothing but the
      // deployment: its own schema, its own boards, its own templates and
      // field definitions (KAIROS-T-0008 provisioning).
      expect(created.schema).toBe(`org_${slug}`);
      expect(created.schema).not.toBe(`org_${ctx.tenant}`);
      expect(created.boards_created).toContain('strategy');
      expect(created.boards_created).toContain('initiatives');
      expect(created.templates_copied).toBeGreaterThan(0);
      expect(created.initial_admin.email).toBe(bob.credentials.email);
      expect(created.initial_admin.role).toBe('admin');
      return {
        organisation: slug,
        schema: created.schema,
        boards_created: (created.boards_created as string[]).join(', '),
        templates_copied: created.templates_copied,
        first_admin: `${created.initial_admin.email} (${created.initial_admin.role})`,
      };
    });

    await step.composeOnly(alice, 'finds it in the register beside the first, both with their schemas intact', COMPOSE_ONLY, async () => {
      const cli = await alice.cli();
      const listed = await cli.json(['admin', 'tenants', 'list', '--limit', '200']);
      const rows = (listed.items ?? listed) as any[];
      const mine = rows.find((t) => t.slug === slug);
      const first = rows.find((t) => t.slug === ctx.tenant);
      expect(mine, 'the new organisation is registered').toBeTruthy();
      expect(first, 'the original is still there').toBeTruthy();
      // `schema_exists` is the drift check an operator relies on: an org
      // row without its schema is a half-provisioned tenant.
      expect(mine.schema_exists).toBe(true);
      expect(first.schema_exists).toBe(true);
      return { organisations: rows.length, both_provisioned: `${first.slug}, ${mine.slug}` };
    });

    await step.composeOnly(bob, 'belongs to both organisations now — and every surface still hands him only one', COMPOSE_ONLY, async () => {
      const token = await bob.token();
      // (API) the X-Tenant header, which is how a caller names an
      // organisation on a header-routed deployment.
      const asked = new Api(token, ctx.server, slug);
      const viaApi = await asked.whoami();
      // (MCP) the same header, the wire an agent speaks.
      const mcp = new McpSession(token, 'bob-asking-for-the-other-org', ctx.server, slug);
      const viaMcp = await mcp.call('whoami');
      // (CLI) `--tenant`, which sets the same header.
      const cli = await bob.cli();
      const viaCli = await cli.json(['whoami', '--tenant', slug]);

      for (const [surface, org] of [
        ['api', viaApi.organization.slug],
        ['cli', viaCli.organization.slug],
      ] as const) {
        expect(org, `${surface} hands bob his own organisation`).toBe(ctx.tenant);
        expect(org).not.toBe(slug);
      }
      expect(viaMcp).toContain(`- organization: ${ctx.tenant}`);
      expect(viaMcp).not.toContain(slug);
      // The control that keeps the observation honest: naming an
      // organisation that does not exist AT ALL answers the same way, so
      // the header is being ignored (KAIROS_SINGLE_TENANT pins this
      // deployment) rather than checked and refused. On a subdomain- or
      // header-routed deployment the same request would be refused with
      // MEMBERSHIP_REQUIRED; either way bob never reaches the other side.
      const nonsense = await new Api(token, ctx.server, 'no-such-organisation').whoami();
      expect(nonsense.organization.slug).toBe(ctx.tenant);
      // And nothing of the second organisation leaks into his world.
      const boards = await (await bob.api()).boards();
      expect(boards.map((b: any) => b.slug).join(' ')).not.toContain(slug);
      return {
        asked_for: slug,
        api: viaApi.organization.slug,
        mcp: ctx.tenant,
        cli: viaCli.organization.slug,
        unknown_org_also_answers: nonsense.organization.slug,
        reason: 'the deployment is pinned to one tenant; the header is not consulted',
      };
    });

    await step.composeOnly(bob, 'is refused the only door between organisations — he cannot even list them', COMPOSE_ONLY, async () => {
      const cli = await bob.cli();
      const api = await bob.api();
      // `/api/admin/tenants` is the one cross-tenant surface in the
      // product (KAIROS-T-0019): outside the tenant middleware, gated on
      // KAIROS_DEPLOYMENT_ADMINS. Being an org admin — which bob now is,
      // of the new organisation — buys nothing here.
      const listed = await api.raw('GET', '/api/admin/tenants');
      expect(listed.status).toBe(403);
      expect(String(listed.body?.error?.message ?? '')).toContain('deployment-admin');
      const creating = await api.raw('POST', '/api/admin/tenants', {
        slug: `${slug}-nope`, name: 'should never exist',
      });
      expect(creating.status).toBe(403);
      const dropping = await api.raw('DELETE', `/api/admin/tenants/${slug}?confirm=true`);
      expect(dropping.status, 'not even for the organisation he administers').toBe(403);
      // The CLI says the same thing, in the words he would read.
      const fromTerminal = await cli.run(['admin', 'tenants', 'list']);
      expect(fromTerminal.code).not.toBe(0);
      expect(`${fromTerminal.stderr}${fromTerminal.stdout}`).toContain('deployment-admin');
      return {
        list: listed.status,
        create: creating.status,
        delete_own_org: dropping.status,
        cli_exit: fromTerminal.code,
        message: String(listed.body?.error?.message ?? '').slice(0, 100),
      };
    });

    await step.composeOnly(alice, 'is refused a careless drop: retiring an organisation takes saying so', COMPOSE_ONLY, async () => {
      const api = await alice.api();
      const res = await api.raw('DELETE', `/api/admin/tenants/${slug}`);
      expect(res.status).toBe(422);
      expect(res.body?.error?.code ?? res.body?.error?.error_code).toBe('CONFIRMATION_REQUIRED');
      // The refusal rolls back cleanly — the organisation is untouched,
      // memberships and all.
      const cli = await alice.cli();
      const listed = await cli.json(['admin', 'tenants', 'list', '--limit', '200']);
      const still = (listed.items ?? listed).find((t: any) => t.slug === slug);
      expect(still, 'the organisation survived the refused drop').toBeTruthy();
      expect(still.schema_exists).toBe(true);
      return {
        status: res.status,
        code: res.body?.error?.code,
        organisation_intact: true,
        message: String(res.body?.error?.message ?? '').slice(0, 110),
      };
    });

    await step.composeOnly(alice, 'retires the second organisation, and the first is untouched', COMPOSE_ONLY, async () => {
      const cli = await alice.cli();
      const api = await alice.api();
      await cli.ok(['admin', 'tenants', 'delete', slug, '--confirm']);
      provisioned = false;
      const listed = await cli.json(['admin', 'tenants', 'list', '--limit', '200']);
      const rows = (listed.items ?? listed) as any[];
      expect(rows.map((t) => t.slug)).not.toContain(slug);
      // Dropped, not hidden: a second delete finds nothing.
      const again = await api.raw('DELETE', `/api/admin/tenants/${slug}?confirm=true`);
      expect(again.status).toBe(404);
      // And the organisation everyone else is working in is exactly where
      // it was — the point of a per-tenant schema.
      const first = rows.find((t) => t.slug === ctx.tenant);
      expect(first?.schema_exists).toBe(true);
      const bobsWorld = await (await bob.cli()).json(['whoami']);
      expect(bobsWorld.organization.slug).toBe(ctx.tenant);
      return {
        retired: slug,
        second_delete: again.status,
        organisations_left: rows.length,
        first_org_intact: ctx.tenant,
      };
    });
  },
);
