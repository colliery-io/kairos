// The harness proof: one persona reaches Kairos on every surface, and a
// compose-only step shows the skip path. Kept in the suite because it is
// the cheapest possible answer to "is the target even up?".
import { expect } from '@playwright/test';
import { journey, step } from '../run/narrate';

journey('smoke', 'Alice reaches Kairos on every surface', { humans: ['alice'] }, async ({ cast }) => {
  const alice = cast.human('alice');

  await step(alice, 'logs into the GUI and sees the boards page', async () => {
    const page = await alice.gui();
    await expect(page.locator('.kairos-board-tile').first()).toBeVisible();
    const tiles = await page.locator('.kairos-board-tile').count();
    return { boards_on_screen: tiles };
  });

  await step(alice, 'runs `kairos whoami` from the CLI', async () => {
    const cli = await alice.cli();
    const me = await cli.json(['whoami']);
    expect(me.user?.email ?? me.email).toBe(alice.credentials.email);
    return { identity: me.user?.email ?? me.email, org_role: me.organization?.role };
  });

  await step(alice, 'opens an MCP session and calls whoami', async () => {
    const mcp = await alice.mcp();
    const text = await mcp.call('whoami');
    expect(text).toContain(alice.credentials.email);
    return { first_line: text.split('\n')[0] };
  });

  await step.composeOnly(alice, 'confirms the deployment-admin routes answer', 'needs a deployment-admin token', async () => {
    const api = await alice.api();
    const tenants = await api.get('/api/admin/tenants');
    return { tenants: (Array.isArray(tenants) ? tenants : tenants.items ?? []).map((t: any) => t.slug) };
  });
});
